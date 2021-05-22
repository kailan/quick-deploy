mod config;
mod github;
mod scdn;
mod templates;

use anyhow::bail;

use serde::{Serialize, Deserialize};
use std::collections::HashMap;

use toml_edit::{Document, value};

use config::{DeployConfig, DeployConfigSpec};
use github::{GitHubClient, GitHubNWO};
use scdn::FastlyClient;
use templates::{DeployContext, ErrorContext, IndexContext, SuccessContext, TemplateRenderer};

use fastly::http::{header, Method, StatusCode};
use fastly::{mime, Error, Request, Response};

/// Stores the user's application state
const STATE_COOKIE: &str = "__Secure-Deploy-Config";

#[derive(Serialize, Deserialize)]
struct ApplicationState {
    pub login: LoginState,
    pub deploy: DeploymentState
}

#[derive(Serialize, Deserialize)]
struct LoginState {
    pub fastly_token: Option<String>,
    pub github_token: Option<String>
}

#[derive(Serialize, Deserialize)]
struct DeploymentState {
    pub src: Option<GitHubNWO>,
    pub dest: Option<GitHubNWO>,
    pub fastly_service_id: Option<GitHubNWO>
}

#[fastly::main]
fn main(req: Request) -> Result<Response, Error> {
    println!(
        "Received request from {}: {} {}",
        req.get_client_ip_addr().unwrap(),
        req.get_method(),
        req.get_path()
    );

    // Initializes the template renderer
    let pages = TemplateRenderer::new();

    match (req.get_method(), req.get_path()) {
        (&Method::GET, "/") => {
            let params: GenerateParams = req.get_query()?;

            let resp = Response::from_status(StatusCode::OK)
                .with_content_type(mime::TEXT_HTML_UTF_8)
                .with_body(pages.render_index_page(IndexContext {
                    button_nwo: params.repository,
                }));

            return Ok(resp);
        }

        (&Method::GET, "/style.css") => {
            return Ok(Response::from_body(include_str!("static/style.css"))
                .with_content_type(mime::TEXT_CSS))
        }

        _ => {}
    }

    match handle_action(req, &pages) {
        Ok(resp) => Ok(resp),
        Err(err) => {
            return Ok(Response::from_status(StatusCode::INTERNAL_SERVER_ERROR)
                .with_content_type(mime::TEXT_HTML_UTF_8)
                .with_body(pages.render_error_page(ErrorContext {
                    message: err.to_string(),
                })))
        }
    }
}

fn handle_action(mut req: Request, pages: &TemplateRenderer) -> Result<Response, Error> {
    // Sets up a GitHub client with app credentials that we can use throughout the request
    let mut gh = GitHubClient::get_default()?;

    // Fetches the cookie header and parses it into a map
    let cookies = get_cookies(&req);

    // Parse state cookie
    let mut state: ApplicationState = match get_cookie(&cookies, STATE_COOKIE) {
        Some(state_cookie) => {
            serde_json::from_str(&String::from_utf8(base64::decode(state_cookie).unwrap()).unwrap()).unwrap()
        },
        None => ApplicationState {
            login: LoginState {
                fastly_token: None,
                github_token: None
            },
            deploy: DeploymentState {
                src: None,
                dest: None,
                fastly_service_id: None
            }
        }
    };

    // Add a user access token to the GitHub client if defined
    gh.user_access_token = match state.login.github_token.as_ref() {
        Some(token) => Some(token.to_string()),
        None => None
    };

    // Fetch the currently active GitHub user, if authenticated
    let gh_user = gh.fetch_user()?;

    // Add a user access token to the Fastly client if defined
    let mut fastly_client = match state.login.fastly_token.as_ref() {
        Some(token) => FastlyClient::from_token(token.to_string()),
        None => FastlyClient::new()
    };

    // Fetch the currently active Fastly user, if authenticated
    let fastly_user = fastly_client.fetch_user()?;

    match (req.get_method(), req.get_path()) {
        (&Method::GET, "/") => {
            let params: GenerateParams = req.get_query()?;

            let resp = Response::from_status(StatusCode::OK)
                .with_content_type(mime::TEXT_HTML_UTF_8)
                .with_body(pages.render_index_page(IndexContext {
                    button_nwo: params.repository,
                }));

            Ok(resp)
        }

        (&Method::POST, "/fork") => {
            // Parse the form params to get repository
            let params: ActionParams = req.take_body_form()?;
            let nwo = &params.repository;

            println!("Forking {}", nwo);

            // Fork the repository
            match gh.fork_repository(&nwo) {
                Ok(repo) => {
                    // Redirect back to deploy flow with the "Active-Fork" cookie set
                    let resp =
                        Response::from_status(StatusCode::FOUND).with_header(header::LOCATION, format!("/{}", nwo));

                    state.deploy.dest = Some(format!("{}+{}/{}", params.repository, repo.owner.login, repo.name));
                    Ok(update_state(resp, &state))
                }
                Err(err) => bail!("Unable to fork repository: {}", err),
            }
        }

        (&Method::POST, "/deploy") => {
            // Parse the form params to get the src and dest repository
            let params: ActionParams = req.take_body_form()?;

            println!("Deploying {}", params.repository);

            // Fetch fastly.toml file from repo
            let manifest_file = match gh.get_file(&params.repository, "fastly.toml")? {
                Some(file) => file,
                None => bail!("The source repository does not contain a fastly.toml file, so cannot be deployed via Quick Deploy")
            };

            println!("Fetched manifest");

            // Parse manifest TOML
            let mut manifest = manifest_file.content.parse::<Document>()?;
            println!("Parsed manifest");

            // Fetch quick-deploy.toml file from repo
            let config_spec = match gh.get_file(&params.repository, "quick-deploy.toml")? {
                Some(file) => DeployConfigSpec::from_toml(&file.content),
                None => bail!("The source repository does not contain a quick-deploy.toml file, so cannot be deployed via Quick Deploy")
            };
            println!("Parsed config specification");

            // Create Fastly service
            let service = fastly_client
                .create_service(&gh_user.unwrap().login, DeployConfig { spec: config_spec })?;
            println!("Service created (ID {})", service.id);

            // Update service ID in manifest
            manifest["service_id"] = value(service.id.to_owned());

            // Generate output manifest
            let output = manifest.to_string();
            println!("Generated updated manifest");

            println!("Enabling actions in forked repository");
            gh.enable_actions(&params.repository)?;

            // Add Fastly API token as repository secret
            println!("Creating FASTLY_API_TOKEN repository secret");
            gh.create_secret(
                &params.repository,
                "FASTLY_API_TOKEN",
                &fastly_client.token.as_ref().unwrap(),
            )?;

            // Update manifest in GitHub repo
            gh.upsert_file(&params.repository, &manifest_file, &output)?;
            println!("Manifest pushed to repository");

            return Ok(Response::from_status(StatusCode::NOT_IMPLEMENTED)
                .with_content_type(mime::TEXT_HTML_UTF_8)
                .with_body(pages.render_success_page(SuccessContext {
                    application_url: format!("https://{}", &service.domain.unwrap()),
                    actions_url: format!("https://github.com/{}/actions", params.repository),
                    repo_nwo: params.repository,
                    service_id: service.id,
                })));
        }

        (&Method::POST, "/auth/fastly") => {
            // Parse the form params to get the Fastly API token
            let form: scdn::AuthParams = req.take_body_form()?;

            // Set the API token in the client
            fastly_client.token = Some(form.token.to_owned());

            // Fetch the current user with the updated token
            let user = match fastly_client.fetch_user()? {
                Some(user) => user,
                None => bail!("Invalid Fastly API token provided"),
            };

            println!(
                "User authenticated via Fastly: {} (cid {})",
                user.name, user.customer_id
            );
            // Redirect to deploy flow with fastly token set
            let resp = Response::from_status(StatusCode::FOUND)
                .with_header(header::LOCATION, get_return_url(&state));

            state.login.fastly_token = fastly_client.token;

            Ok(update_state(resp, &state))
        }

        // Redirect to GitHub authorization flow
        (&Method::GET, "/oauth/github") => Ok(Response::from_status(StatusCode::FOUND)
            .with_header(header::LOCATION, gh.get_authorize_url())),

        // Handle callbacks from GitHub authorization flow
        (&Method::GET, "/oauth/github/callback") => match req.get_query::<github::AuthParams>() {
            Ok(auth) => {
                // Request an access token using the received code
                let token = gh.get_access_token_from_params(auth)?;

                // Set the access token in the GitHub client
                gh.user_access_token = Some(token.to_owned());

                println!("User authenticated via GitHub");
                // Return to deploy flow with gh token set
                let resp = Response::from_status(StatusCode::FOUND)
                    .with_header(header::LOCATION, get_return_url(&state));

                state.login.github_token = Some(token);

                Ok(update_state(resp, &state))
            }
            Err(_) => Ok(Response::from_status(StatusCode::BAD_REQUEST)
                .with_body_str("No auth 'code' param provided\n")),
        },

        (&Method::GET, "/style.css") => {
            Ok(Response::from_body(include_str!("static/style.css"))
                .with_content_type(mime::TEXT_CSS))
        }

        // Serve deploy page on repository routes, e.g. "/abc/def"
        (&Method::GET, _) if req.get_path().matches("/").count() == 2 => {
            let path = req.get_path();
            let src_nwo = &path[1..path.len()];

            let dest_repository: Option<String> = match state.deploy.dest.as_ref() {
                Some(state) => {
                    let mut parts = state.split("+");
                    if parts.next().unwrap() != src_nwo {
                        None
                    } else {
                        Some(parts.next().unwrap().to_string())
                    }
                }
                None => None,
            };

            println!("Fetching github.com/{}", src_nwo);

            // Fetch the repo using the ANONYMOUS github client, so we only fetch public repos
            // and are able to cache them.
            let repo = match gh.anonymous().fetch_repository(src_nwo)? {
                Some(repo) => repo,
                None => bail!("No repository was found at github.com{}", path),
            };

            let can_deploy =
                gh_user.is_some() && fastly_user.is_some() && dest_repository.is_some();

            // Fetch quick-deploy.toml file from repo
            let config_spec = match gh.get_file(&src_nwo, "quick-deploy.toml")? {
                Some(file) => Some(DeployConfigSpec::from_toml(&file.content)),
                None => None,
            };

            let resp = Response::from_status(StatusCode::OK)
                .with_content_type(mime::TEXT_HTML_UTF_8)
                .with_body(pages.render_deploy_page(DeployContext {
                    src: repo,
                    can_deploy,
                    can_fork: gh_user.is_some() && !dest_repository.is_some(),
                    github_user: gh_user,
                    fastly_user,
                    dest_nwo: dest_repository,
                    config_spec,
                }));

            state.deploy.src = Some(src_nwo.to_string());

            Ok(update_state(resp, &state))
        }

        // Catch all other requests and return a 404.
        _ => Ok(Response::from_status(StatusCode::NOT_FOUND)
            .with_body_str("The page you requested could not be found\n")),
    }
}

#[derive(Deserialize)]
struct GenerateParams {
    repository: Option<String>,
}

#[derive(Deserialize)]
struct ActionParams {
    repository: String,
}

fn get_return_url(state: &ApplicationState) -> String {
    format!("/{}", state.deploy.src.as_ref().unwrap_or(&"".to_string()))
}

fn update_state(resp: Response, state: &ApplicationState) -> Response {
    resp.with_header(
        header::SET_COOKIE,
        format!("{}={}; Secure; HttpOnly; Path=/;", STATE_COOKIE, base64::encode(serde_json::to_string(state).unwrap())),
    )
}

fn get_cookie(cookies: &HashMap<&str, &str>, key: &str) -> Option<String> {
    match cookies.get(key) {
        Some(value) => Some(value.to_string()),
        None => None,
    }
}

fn get_cookies(req: &Request) -> HashMap<&str, &str> {
    match req.get_header(header::COOKIE) {
        Some(cookie) => parse_cookies_to_map(cookie.to_str().unwrap()),
        None => HashMap::new(),
    }
}

fn parse_cookies_to_map(value: &str) -> HashMap<&str, &str> {
    let mut jar = HashMap::new();
    for cookie in value.split(';') {
        let mut split = cookie.trim().split("=");
        jar.insert(split.next().unwrap(), split.next().unwrap());
    }
    jar
}
