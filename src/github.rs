use anyhow::{bail, Result};
use fastly::{
  http::{header, Method, StatusCode},
  Dictionary, Request,
};
use sealed_box::PublicKey;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::convert::TryInto;

const AUTH_BACKEND: &str = "github.com";
const API_BACKEND: &str = "api.github.com";
const USER_AGENT: &str = "Quick Deploy (@kailan)";

pub type GitHubNWO = String;

pub struct GitHubClient {
  client_id: String,
  client_secret: String,

  pub user_access_token: Option<String>,
}

impl GitHubClient {
  pub fn get_default() -> Result<GitHubClient> {
    GitHubClient::from_dictionary("github_auth")
  }

  pub fn from_dictionary(dictionary_name: &str) -> Result<GitHubClient> {
    let dictionary = Dictionary::open(dictionary_name);

    Ok(GitHubClient {
      client_id: dictionary.get("client_id").unwrap(),
      client_secret: dictionary.get("client_secret").unwrap(),
      user_access_token: None,
    })
  }

  pub fn anonymous(&self) -> GitHubClient {
    GitHubClient {
      client_id: self.client_id.to_owned(),
      client_secret: self.client_secret.to_owned(),
      user_access_token: None,
    }
  }

  pub fn github_request(&self, req: Request) -> Request {
    let mut req = req
      .with_header(header::USER_AGENT, USER_AGENT)
      .with_header(header::ACCEPT, "application/vnd.github.baptiste-preview+json");
    if let Some(token) = &self.user_access_token {
      req.set_header(header::AUTHORIZATION, format!("token {}", token));
      req.set_pass(true);
    }
    req
  }

  pub fn get_authorize_url(&self) -> String {
    format!(
      "https://github.com/login/oauth/authorize?client_id={}&scope=repo%20workflow",
      &self.client_id
    )
  }

  pub fn get_access_token_from_params(&mut self, params: AuthParams) -> Result<String> {
    let req = self
      .github_request(Request::new(
        Method::POST,
        "https://github.com/login/oauth/access_token",
      ))
      .with_pass(true)
      .with_body_json(&AccessTokenRequest::from_code(self, params.code))?;

    let token: AccessTokenResponse = req.send(AUTH_BACKEND)?.take_body_json()?;
    Ok(token.access_token)
  }

  pub fn fetch_user(&self) -> Result<Option<GitHubUser>> {
    if self.user_access_token == None {
      return Ok(None);
    }

    let req = self.github_request(Request::new(Method::GET, "https://api.github.com/user"));
    let mut resp = req.send(API_BACKEND)?;
    match resp.take_body_json::<GitHubUser>() {
      Ok(user) => Ok(Some(user)),
      Err(err) => bail!("Unable to fetch logged in user from GitHub: {}", err),
    }
  }

  pub fn fetch_repository(&self, nwo: &str) -> Result<Option<GitHubRepository>> {
    let req = self.github_request(Request::new(
      Method::GET,
      format!("https://api.github.com/repos/{}", nwo),
    )).with_ttl(60 * 60 * 3); // The only data used from here is star + fork count so we can cache for a while
    let mut resp = req.send(API_BACKEND)?;

    match resp.get_status() {
      StatusCode::OK => Ok(Some(resp.take_body_json()?)),

      StatusCode::NOT_FOUND => Ok(None),

      _ => bail!(
        "Unable to fetch GitHub repository {}: {}",
        nwo,
        resp.take_body_str()
      ),
    }
  }

  pub fn fork_repository(&self, nwo: &str, dst_name: &str) -> Result<GitHubRepository> {
    let body = json!({"name": dst_name});
    let req = self.github_request(Request::new(
      Method::POST,
      format!("https://api.github.com/repos/{}/generate", nwo),
    )).with_pass(true).with_body_json(&body).unwrap();
    let mut resp = req.send(API_BACKEND)?;
    match resp.get_status() {
      StatusCode::CREATED => Ok(resp.take_body_json::<GitHubRepository>()?),
      _ => bail!("Unable to fork GitHub repository {}: {}", nwo, resp.take_body_str())
    }
  }

  pub fn enable_actions(&self, nwo: &str) -> Result<()> {
    let req = self.github_request(
      Request::new(
        Method::PUT,
        format!(
          "https://api.github.com/repos/{}/actions/workflows/deploy/enable",
          nwo
        ),
      )
      .with_pass(true),
    );
    req.send(API_BACKEND)?;
    Ok(())
  }

  pub fn get_file(&self, nwo: &str, path: &str) -> Result<Option<GitHubFile>> {
    let req = self.github_request(Request::new(
      Method::GET,
      format!("https://api.github.com/repos/{}/contents/{}", nwo, path),
    ));
    let mut resp = req.send(API_BACKEND)?;
    match resp.get_status() {
      StatusCode::OK => {
        let mut file: GitHubFile = resp.take_body_json()?;
        file.content = String::from_utf8(base64::decode(file.content.replace('\n', ""))?)?;
        Ok(Some(file))
      }

      StatusCode::NOT_FOUND => Ok(None),

      _ => bail!(
        "Unable to fetch {} file from GitHub repository {}: {}",
        path,
        nwo,
        resp.take_body_str()
      ),
    }
  }

  pub fn upsert_file(&self, nwo: &str, file: &GitHubFile, content: &str) -> Result<()> {
    let mut req = self
      .github_request(Request::new(
        Method::PUT,
        format!(
          "https://api.github.com/repos/{}/contents/{}",
          nwo, file.path
        ),
      ))
      .with_pass(true);
    req.set_body_json(&FileUpdateRequest {
      content: base64::encode(content),
      message: "Service provisioning via deploy.edgecompute.app".to_string(),
      sha: file.sha.to_owned(),
    })?;
    req.send(API_BACKEND)?;
    Ok(())
  }

  pub fn get_repository_public_key(&self, nwo: &str) -> Result<(PublicKey, String)> {
    let req = self.github_request(Request::new(
      Method::GET,
      format!(
        "https://api.github.com/repos/{}/actions/secrets/public-key",
        nwo
      ),
    ));
    let mut resp = req.send(API_BACKEND)?;
    match resp.take_body_json::<PublicKeyResponse>() {
      Ok(body) => {
        let key = base64::decode(body.key)?;
        Ok((key.try_into().unwrap(), body.key_id))
      }
      Err(err) => bail!(err),
    }
  }

  pub fn create_secret(&self, nwo: &str, key: &str, value: &str) -> Result<()> {
    let (pk, key_id) = self.get_repository_public_key(nwo)?;

    let encrypted_value = sealed_box::seal(value, pk);

    let mut req = self
      .github_request(Request::new(
        Method::PUT,
        format!(
          "https://api.github.com/repos/{}/actions/secrets/{}",
          nwo, key
        ),
      ))
      .with_pass(true);
    req.set_body_json(&CreateSecretRequest {
      key_id,
      encrypted_value: base64::encode(encrypted_value),
    })?;
    match req.send(API_BACKEND) {
      Ok(mut resp) => match resp.get_status() {
        StatusCode::CREATED | StatusCode::NO_CONTENT => Ok(()),
        _ => {
          bail!("Unable to create secret: {}", resp.take_body_str())
        }
      },
      Err(err) => bail!(err),
    }
  }
}

#[derive(Deserialize)]
struct PublicKeyResponse {
  key: String,
  key_id: String,
}

#[derive(Serialize)]
struct CreateSecretRequest {
  encrypted_value: String,
  key_id: String,
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
  pub is_template: bool
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
