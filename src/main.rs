mod github;
mod scdn;
mod templates;

use serde::Deserialize;
use std::collections::HashMap;

use toml::Value;

use github::GitHubClient;
use scdn::FastlyClient;
use templates::{DeployContext, ErrorContext, SuccessContext, TemplateRenderer};

use fastly::http::{header, Method, StatusCode};
use fastly::{mime, Error, Request, Response};

#[fastly::main]
fn main(mut req: Request) -> Result<Response, Error> {
    // Sets up a GitHub client with app credentials that we can use throughout the request
    let mut gh = GitHubClient::get_default();

    // Initializes the template renderer
    let mut pages = TemplateRenderer::new();

    // Fetches the cookie header and parses it into a map
    let cookies = get_cookies(&req);

    // Fetch the "Return-To" cookie to determine where to send the user after auth
    let return_location = get_cookie(&cookies, "Return-To").unwrap_or("/".to_string());

    // Fetch the "Active-Fork" cookie to determine if the repo has been forked
    let mut active_fork = get_cookie(&cookies, "Active-Fork");

    // Add a user access token to the GitHub client if defined
    gh.user_access_token = get_cookie(&cookies, "__Secure-GH-Token");

    // Fetch the currently active GitHub user, if authenticated
    let gh_user = gh.fetch_user();

    // Fetch the value of the Fastly auth token and initialize a new API client
    let mut fastly_client = if let Some(token) = get_cookie(&cookies, "__Secure-Fastly-Token") {
        FastlyClient::from_token(token)
    } else {
        FastlyClient::new()
    };

    // Fetch the currently active Fastly user, if authenticated
    let fastly_user = fastly_client.fetch_user();

    match (req.get_method(), req.get_path()) {
        (&Method::POST, "/fork") => {
            // Parse the form params to get repository
            let params: ForkParams = req.take_body_form().unwrap();
            let nwo = &params.repository;

            // Fork the repository
            match gh.fork_repository(&nwo) {
                Some(repo) => {
                    // Redirect back to deploy flow with the "Active-Fork" cookie set
                    let resp = Response::from_status(StatusCode::FOUND)
                        .with_header(header::LOCATION, nwo);
                    Ok(set_cookie(
                        resp,
                        "Active-Fork",
                        &format!("{}+{}/{}", params.repository, repo.owner.login, repo.name),
                    ))
                }
                None => Ok(Response::from_status(StatusCode::NOT_FOUND)
                    .with_content_type(mime::TEXT_HTML_UTF_8)
                    .with_body(pages.render_error_page(ErrorContext {
                        message: "Unable to fork repository".to_string(),
                    }))),
            }
        }

        (&Method::POST, "/deploy") => {
            // Parse the form params to get the src and dest repository
            let params: DeployParams = req.take_body_form().unwrap();

            println!("Deploying {}", params.dest);

            // Fetch fastly.toml file from repo
            let manifest = match gh.get_file(&params.dest, "fastly.toml") {
                Some(file) => file,
                None => return Ok(Response::from_status(StatusCode::NOT_FOUND)
                .with_content_type(mime::TEXT_HTML_UTF_8)
                .with_body(pages.render_error_page(ErrorContext {
                    message: "Unable to read manifest file from repository. Either the forked repo is not a C@E project, or your fork is not yet ready.".to_string(),
                })))
            };
            println!("Fetched manifest");

            // Parse manifest TOML
            let mut value = manifest.content.parse::<Value>().unwrap();
            let table = value.as_table_mut().unwrap();
            println!("Parsed manifest");

            // Create Fastly service
            let service = fastly_client.create_service(&gh_user.unwrap().login).unwrap();
            println!("Service created (ID {})", service.id);

            // Update service ID in manifest
            table.insert("service_id".to_string(), Value::String(service.id.to_owned()));

            // Generate output manifest
            let output = toml::to_string(&table).unwrap();
            println!("Generated updated manifest");

            println!("Enabling actions in forked repository");
            gh.enable_actions(&params.dest);

            // Update manifest in GitHub repo
            gh.upsert_file(&params.dest, &manifest, &output);
            println!("Manifest pushed to repository");

            return Ok(Response::from_status(StatusCode::NOT_IMPLEMENTED)
                .with_content_type(mime::TEXT_HTML_UTF_8)
                .with_body(pages.render_success_page(SuccessContext {
                    application_url: format!("https://{}", &service.domain.unwrap()),
                    actions_url: format!("https://github.com/{}/actions", params.dest),
                    repo_nwo: params.dest,
                    service_id: service.id
                })))
        },

        (&Method::POST, "/auth/fastly") => {
            // Parse the form params to get the Fastly API token
            let form: scdn::AuthParams = req.take_body_form().unwrap();

            // Set the API token in the client
            fastly_client.token = Some(form.token.to_owned());

            // Fetch the current user with the updated token
            match fastly_client.fetch_user() {
                Some(_user) => {
                    // Redirect to deploy flow with fastly token set
                    let resp = Response::from_status(StatusCode::FOUND)
                        .with_header(header::LOCATION, return_location);
                    Ok(set_cookie(resp, "__Secure-Fastly-Token", &form.token))
                }
                None => {
                    // Redirect to deploy flow with no token
                    // TODO: error handling
                    let resp = Response::from_status(StatusCode::FOUND)
                        .with_header(header::LOCATION, return_location);
                    Ok(resp)
                }
            }
        }

        // Redirect to GitHub authorization flow
        (&Method::GET, "/oauth/github") => Ok(Response::from_status(StatusCode::FOUND)
            .with_header(header::LOCATION, gh.get_authorize_url())),

        // Handle callbacks from GitHub authorization flow
        (&Method::GET, "/oauth/github/callback") => match req.get_query::<github::AuthParams>() {
            Ok(auth) => {
                // Request an access token using the received code
                let token = gh.get_access_token_from_params(auth);

                // Set the access token in the GitHub client
                gh.user_access_token = Some(token.to_owned());

                // Return to deploy flow with gh token set
                let resp = Response::from_status(StatusCode::FOUND)
                    .with_header(header::LOCATION, return_location);
                Ok(set_cookie(resp, "__Secure-GH-Token", &token))
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

            let dest_repository: Option<String> = match active_fork {
                Some(state) => {
                    let mut parts = state.split("+");
                    if parts.next().unwrap() != src_nwo {
                        None
                    } else {
                        Some(parts.next().unwrap().to_string())
                    }
                },
                None => None
            };

            // Fetch the repo using the ANONYMOUS github client, so we only fetch public repos
            // and are able to cache them.
            let repo = match gh.anonymous().fetch_repository(src_nwo) {
                Some(repo) => repo,
                None => {
                    return Ok(Response::from_status(StatusCode::NOT_FOUND)
                        .with_content_type(mime::TEXT_HTML_UTF_8)
                        .with_body(pages.render_error_page(ErrorContext {
                            message: format!("No repository was found at github.com{}", path),
                        })));
                }
            };

            let mut resp = Response::from_status(StatusCode::OK)
                .with_content_type(mime::TEXT_HTML_UTF_8)
                .with_body(pages.render_deploy_page(DeployContext {
                    src: repo,
                    can_deploy: gh_user.is_some() && fastly_user.is_some() && dest_repository.is_some(),
                    can_fork: gh_user.is_some() && !dest_repository.is_some(),
                    github_user: gh_user,
                    fastly_user,
                    dest_nwo: dest_repository
                }));

            resp = set_cookie(resp, "Return-To", req.get_path());

            Ok(resp)
        }

        // Catch all other requests and return a 404.
        _ => Ok(Response::from_status(StatusCode::NOT_FOUND)
            .with_body_str("The page you requested could not be found\n")),
    }
}

#[derive(Deserialize)]
struct ForkParams {
    repository: String,
}

#[derive(Deserialize)]
struct DeployParams {
    src: String,
    dest: String,
}

fn set_cookie(resp: Response, key: &str, value: &str) -> Response {
    resp.with_header(
        header::SET_COOKIE,
        format!("{}={}; Secure; HttpOnly; Path=/;", key, value),
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
