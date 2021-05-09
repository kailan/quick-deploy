use fastly::{
  http::{header, Method, StatusCode},
  Dictionary, Request,
};
use serde::{Deserialize, Serialize};

const AUTH_BACKEND: &str = "github.com";
const API_BACKEND: &str = "api.github.com";
const USER_AGENT: &str = "Quick Deploy (@kailan)";

pub struct GitHubClient {
  client_id: String,
  client_secret: String,

  pub user_access_token: Option<String>,
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
      user_access_token: None,
    }
  }

  pub fn anonymous(self) -> GitHubClient {
    GitHubClient {
      client_id: self.client_id,
      client_secret: self.client_secret,
      user_access_token: None,
    }
  }

  pub fn github_request(&self, req: Request) -> Request {
    let mut req = req
      .with_header(header::USER_AGENT, USER_AGENT)
      .with_header(header::ACCEPT, "application/json");
    if let Some(token) = &self.user_access_token {
      req.set_header(header::AUTHORIZATION, format!("token {}", token));
      req.set_pass(true);
    }
    req
  }

  pub fn get_authorize_url(&self) -> String {
    format!(
      "https://github.com/login/oauth/authorize?client_id={}&scope=repo",
      &self.client_id
    )
  }

  pub fn get_access_token_from_params(&mut self, params: AuthParams) -> String {
    let req = self
      .github_request(Request::new(
        Method::POST,
        "https://github.com/login/oauth/access_token",
      ))
      .with_body_json(&AccessTokenRequest::from_code(self, params.code))
      .unwrap()
      .with_pass(true);

    let token: AccessTokenResponse = req.send(AUTH_BACKEND).unwrap().take_body_json().unwrap();
    token.access_token
  }

  pub fn fetch_user(&self) -> Option<GitHubUser> {
    if self.user_access_token == None {
      return None;
    }

    let req = self.github_request(Request::new(Method::GET, "https://api.github.com/user"));
    let mut resp = req.send(API_BACKEND).unwrap();
    match resp.take_body_json::<GitHubUser>() {
      Ok(user) => Some(user),
      Err(_) => None,
    }
  }

  pub fn fetch_repository(&self, nwo: &str) -> Option<GitHubRepository> {
    let req = self.github_request(Request::new(
      Method::GET,
      format!("https://api.github.com/repos/{}", nwo),
    ));
    let mut resp = req.send(API_BACKEND).unwrap();
    match resp.take_body_json::<GitHubRepository>() {
      Ok(repo) => Some(repo),
      Err(_) => None,
    }
  }

  pub fn fork_repository(&self, nwo: &str) -> Option<GitHubRepository> {
    let req = self.github_request(Request::new(
      Method::POST,
      format!("https://api.github.com/repos/{}/forks", nwo),
    ));
    let mut resp = req.send(API_BACKEND).unwrap();
    match resp.take_body_json::<GitHubRepository>() {
      Ok(repo) => Some(repo),
      Err(_) => None,
    }
  }

  pub fn enable_actions(&self, nwo: &str) {
    let req = self
      .github_request(Request::new(
        Method::PUT,
        format!("https://api.github.com/repos/{}/actions/permissions", nwo),
      ))
      .with_body_json(&ActionsPermissionsRequest { enabled: true })
      .unwrap();
    req.send(API_BACKEND).unwrap();
  }

  pub fn get_file(&self, nwo: &str, path: &str) -> Option<GitHubFile> {
    let req = self.github_request(Request::new(
      Method::GET,
      format!("https://api.github.com/repos/{}/contents/{}", nwo, path),
    ));
    let mut resp = req.send(API_BACKEND).unwrap();
    match resp.get_status() {
      StatusCode::OK => {
        let mut file: GitHubFile = resp.take_body_json().unwrap();
        file.content =
          String::from_utf8(base64::decode(file.content.replace('\n', "")).unwrap()).unwrap();
        Some(file)
      }
      _ => None,
    }
  }

  pub fn upsert_file(&self, nwo: &str, file: &GitHubFile, content: &str) {
    let mut req = self
      .github_request(Request::new(
        Method::PUT,
        format!(
          "https://api.github.com/repos/{}/contents/{}",
          nwo, file.path
        ),
      ))
      .with_pass(true);
    req
      .set_body_json(&FileUpdateRequest {
        content: base64::encode(content),
        message: "Service provisioning via deploy.edgecompute.app".to_string(),
        sha: file.sha.to_owned(),
      })
      .unwrap();
    req.send(API_BACKEND).unwrap();
  }
}

#[derive(Serialize)]
struct ActionsPermissionsRequest {
  enabled: bool,
}

#[derive(Serialize)]
struct FileUpdateRequest {
  content: String,
  message: String,
  sha: String,
}

#[derive(Deserialize)]
pub struct GitHubFile {
  path: String,
  pub content: String,
  sha: String,
}

#[derive(Deserialize, Serialize)]
pub struct GitHubRepository {
  pub name: String,
  pub default_branch: String,
  pub owner: GitHubUser,
  pub forks_count: i32,
  pub stargazers_count: i32,
}

#[derive(Deserialize, Serialize)]
pub struct GitHubUser {
  pub login: String,
  pub name: Option<String>,
}

#[derive(Deserialize)]
pub struct AuthParams {
  pub code: String,
}

#[derive(Serialize)]
struct AccessTokenRequest {
  client_id: String,
  client_secret: String,
  code: String,
}

impl AccessTokenRequest {
  fn from_code(client: &GitHubClient, code: String) -> AccessTokenRequest {
    AccessTokenRequest {
      client_id: client.client_id.to_owned(),
      client_secret: client.client_secret.to_owned(),
      code: code,
    }
  }
}

#[derive(Deserialize)]
struct AccessTokenResponse {
  access_token: String,
}
