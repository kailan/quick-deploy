mod github;
mod scdn;
mod templates;

use std::collections::HashMap;

use github::GitHubClient;
use scdn::FastlyClient;
use templates::{DeployContext, SourceRepository, TemplateRenderer};

use fastly::http::{header, StatusCode};
use fastly::{mime, Error, Request, Response};

#[fastly::main]
fn main(mut req: Request) -> Result<Response, Error> {
    let mut gh = GitHubClient::get_default();
    let mut pages = TemplateRenderer::new();

    let cookies = get_cookies(&req);
    gh.user_access_token = get_cookie(&cookies, "__Secure-GH-Token");

    let gh_user = gh.fetch_user();

    let mut fastly_client = if let Some(token) = get_cookie(&cookies, "__Secure-Fastly-Token") {
        FastlyClient::from_token(token)
    } else {
        FastlyClient::new()
    };

    let fastly_user = fastly_client.fetch_user();

    match req.get_path() {
        // If request is to the `/` path, send a default response.
        "/fastly/compute-starter-kit-rust-static-content" => Ok(Response::from_status(StatusCode::OK)
            .with_content_type(mime::TEXT_HTML_UTF_8)
            .with_body(pages.render_deploy_page(DeployContext {
                src: SourceRepository {
                    owner: "fastly".to_string(),
                    name: "compute-starter-kit-rust-static-content".to_string(),
                },
                can_deploy: gh_user.is_some() && fastly_user.is_some(),
                github_user: gh_user,
                fastly_user,
            }))),

        "/deploy" => {
            Ok(Response::from_status(StatusCode::NOT_IMPLEMENTED)
            .with_body_str("Endpoint not implemented\n"))
        },

        "/auth/fastly" => {
            let form: scdn::AuthParams = req.take_body_form().unwrap();

            fastly_client.token = Some(form.token.to_owned());
            match fastly_client.fetch_user() {
                Some(user) => {
                    let resp = Response::from_status(StatusCode::FOUND).with_header(header::LOCATION, "/fastly/compute-starter-kit-rust-static-content");
                    Ok(set_cookie(resp, "__Secure-Fastly-Token", &form.token))
                },
                None => {
                    let mut resp = Response::from_status(StatusCode::UNAUTHORIZED).with_header(header::LOCATION, "/fastly/compute-starter-kit-rust-static-content");
                    Ok(resp)
                }
            }
        },

        "/oauth/github" => Ok(Response::from_status(StatusCode::FOUND)
            .with_header(header::LOCATION, gh.get_authorize_url())),

        "/oauth/github/callback" => match req.get_query::<github::AuthParams>() {
            Ok(auth) => {
                let token = gh.get_access_token_from_params(auth);
                gh.user_access_token = Some(token.to_owned());

                let resp = Response::from_status(StatusCode::FOUND).with_header(header::LOCATION, "/fastly/compute-starter-kit-rust-static-content");
                Ok(set_cookie(resp, "__Secure-GH-Token", &token))
            }
            Err(e) => Ok(Response::from_status(StatusCode::BAD_REQUEST)
                .with_body_str("No auth 'code' param provided\n")),
        },

        "/style.css" => Ok(Response::from_body(include_str!("static/style.css")).with_content_type(mime::TEXT_CSS)),

        // Catch all other requests and return a 404.
        _ => Ok(Response::from_status(StatusCode::NOT_FOUND)
            .with_body_str("The page you requested could not be found\n")),
    }
}

fn set_cookie(resp: Response, key: &str, value: &str) -> Response {
    resp.with_header(header::SET_COOKIE, format!("{}={}; Secure; HttpOnly; Path=/;", key, value))
}

fn get_cookie(cookies: &HashMap<&str, &str>, key: &str) -> Option<String> {
    match cookies.get(key) {
        Some(value) => Some(value.to_string()),
        None => None
    }
}

fn get_cookies(req: &Request) -> HashMap<&str, &str> {
    match req.get_header(header::COOKIE) {
        Some(cookie) => {
            parse_cookies_to_map(cookie.to_str().unwrap())
        },
        None => HashMap::new()
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
