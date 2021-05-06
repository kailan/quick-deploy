use fastly::{Request, Dictionary, http::{Method, header}};
use serde::{Serialize, Deserialize};

const AUTH_BACKEND: &str = "github.com";
const API_BACKEND: &str = "api.github.com";
const USER_AGENT: &str = "Quick Deploy (@kailan)";

pub struct GitHubClient {
  client_id: String,
  client_secret: String,

  pub user_access_token: Option<String>
}

impl GitHubClient {
  pub fn get_default() -> GitHubClient {
    GitHubClient::from_dictionary("github_auth")
  }

  pub fn from_dictionary(dictionary_name: &str) -> GitHubClient {
    let dictionary = Dictionary::open(dictionary_name);

    GitHubClient {
      client_id: dictionary.get("client_id").unwrap(),
      client_secret: dictionary.get("client_secret").unwrap(),
      user_access_token: None
    }
  }

  pub fn github_request(&self, req: Request) -> Request {
    req.with_header(header::USER_AGENT, USER_AGENT).with_header(header::ACCEPT, "application/json")
  }

  pub fn get_authorize_url(&self) -> String {
    format!("https://github.com/login/oauth/authorize?client_id={}&scope=repo", &self.client_id)
  }

  pub fn get_access_token_from_params(&mut self, params: AuthParams) -> String {
    let req = self.github_request(Request::new(Method::POST, "https://github.com/login/oauth/access_token"))
      .with_body_json(&AccessTokenRequest::from_code(self, params.code)).unwrap();

    let token: AccessTokenResponse = req.send(AUTH_BACKEND).unwrap().take_body_json().unwrap();
    token.access_token
  }
}

#[derive(Deserialize)]
pub struct AuthParams {
  pub code: String
}

#[derive(Serialize)]
struct AccessTokenRequest {
  client_id: String,
  client_secret: String,
  code: String
}

impl AccessTokenRequest {
  fn from_code(client: &GitHubClient, code: String) -> AccessTokenRequest {
    AccessTokenRequest {
      client_id: client.client_id.to_owned(),
      client_secret: client.client_secret.to_owned(),
      code: code
    }
  }
}

#[derive(Deserialize)]
struct AccessTokenResponse {
  access_token: String
}
