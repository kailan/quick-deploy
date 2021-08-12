use crate::config::BackendSpec;
use crate::config::DeployConfig;
use anyhow::{bail, Result};
use fastly::http::StatusCode;
use fastly::{
  http::{header, Method},
  Request,
};
use serde::{Deserialize, Serialize};

const USER_AGENT: &str = "Quick Deploy (@kailan)";
const API_BACKEND: &str = "api.fastly.com";

#[derive(Serialize, Deserialize)]
pub struct FastlyClient {
  pub token: Option<String>,
}

impl FastlyClient {
  pub fn from_token(token: String) -> FastlyClient {
    FastlyClient { token: Some(token) }
  }

  pub fn new() -> FastlyClient {
    FastlyClient { token: None }
  }

  pub fn fastly_request(&self, req: Request) -> Result<Request> {
    if self.token == None {
      bail!("No Fastly API token set");
    }

    Ok(req
      .with_header(header::USER_AGENT, USER_AGENT)
      .with_header(header::HOST, "api.fastly.com")
      .with_header(header::ACCEPT, "application/json")
      .with_header("Fastly-Key", self.token.as_ref().unwrap())
      .with_pass(true))
  }

  pub fn fetch_user(&self) -> Result<Option<FastlyUser>> {
    if self.token == None {
      return Ok(None);
    }

    let req = self.fastly_request(Request::new(
      Method::GET,
      "https://api.fastly.com/current_user",
    ))?;
    let mut resp = req.send(API_BACKEND)?;
    match resp.get_status() {
      StatusCode::OK => Ok(Some(resp.take_body_json::<FastlyUser>()?)),
      _ => bail!("Unable to authenticate with Fastly")
    }
  }

  pub fn create_service(&self, slug: &str, mut deploy: DeployConfig) -> Result<FastlyService> {
    let domain = format!("{}.edgecompute.app", slug);

    // Create a service
    let servreq = FastlyServiceRequest {
      service_type: "wasm".to_string(),
      name: format!("{} via Quick Deploy", slug).to_string(),
    };

    let req = self
      .fastly_request(Request::new(Method::POST, "https://api.fastly.com/service"))?
      .with_pass(true)
      .with_body_json(&servreq)?;
    let mut resp = req.send(API_BACKEND)?;

    let mut service = match resp.get_status() {
      StatusCode::OK => resp.take_body_json::<FastlyService>()?,
      _ => bail!("Error while creating service: {}", resp.take_body_str())
    };
    println!("Created service {}", service.id);

    // Create a domain
    let req = self
      .fastly_request(Request::new(
        Method::POST,
        format!(
          "https://api.fastly.com/service/{}/version/1/domain",
          service.id
        ),
      ))?
      .with_pass(true)
      .with_body_json(&FastlyDomain { name: domain })?;
    let mut resp = req.send(API_BACKEND)?;

    let domain = match resp.take_body_json::<FastlyDomain>() {
      Ok(domain) => domain,
      Err(err) => bail!("Error while creating domain: {}", err),
    };
    println!("Created domain {}", domain.name);

    service.domain = Some(domain.name);

    // Create backends
    if deploy.spec.backends.len() == 0 {
      deploy.spec.backends.push(BackendSpec {
        name: "127.0.0.1".to_string(),
        address: "127.0.0.1".to_string(),
        port: None,
        prompt: None,
      });
    }

    for backend in deploy.spec.backends {
      let req = match self
        .fastly_request(Request::new(
          Method::POST,
          format!(
            "https://api.fastly.com/service/{}/version/1/backend",
            service.id
          ),
        ))?
        .with_pass(true)
        .with_body_json(&FastlyBackend {
          name: backend.name.to_owned(),
          address: backend.address,
          port: backend.port.unwrap_or(80),
        }) {
        Ok(req) => req,
        Err(err) => bail!("Error while creating backend {}: {}", backend.name, err),
      };
      req.send(API_BACKEND)?;
      println!("Created backend {}", backend.name);
    }

    for dict in deploy.spec.dictionaries {
      // Create dictionary
      let req = match self
        .fastly_request(Request::new(
          Method::POST,
          format!(
            "https://api.fastly.com/service/{}/version/1/dictionary",
            service.id
          ),
        ))?
        .with_pass(true)
        .with_body_json(&FastlyDictionary {
          id: None,
          name: dict.name.to_owned(),
        }) {
        Ok(req) => req,
        Err(err) => bail!("Error while creating dictionary {}: {}", dict.name, err),
      };
      let mut resp = req.send(API_BACKEND)?;
      let created_dict: FastlyDictionary = resp.take_body_json()?;
      println!("Created dictionary {}", dict.name);

      let mut entries: Vec<FastlyDictionaryItemAction> = vec![];
      for entry in dict.items {
        entries.push(FastlyDictionaryItemAction {
          op: "create".to_string(),
          item_key: entry.key.to_owned(),
          item_value: match deploy.params.get(&format!("dict.{}.{}", dict.name, entry.key)) {
            Some(value) => value.to_string(),
            None => match entry.value {
              Some(default) => default,
              None => bail!("No value provided for dict key {}", entry.key)
            }
          },
        });
      }

      let entry_count = entries.len();

      match self
        .fastly_request(Request::new(
          Method::PATCH,
          format!(
            "https://api.fastly.com/service/{}/dictionary/{}/items",
            service.id,
            created_dict.id.unwrap()
          ),
        ))?
        .with_pass(true)
        .with_body_json(&FastlyDictionaryUpdateRequest { items: entries })?
        .send(API_BACKEND)
      {
        Ok(_) => {
          println!("Populated dictionary {} with {} items", dict.name, entry_count);
        },
        Err(err) => bail!(
          "Error while adding items to dictionary {}: {:?}",
          dict.name,
          err
        ),
      };
    }

    Ok(service)
  }

  pub fn check_service_deployment(&self, service_id: &str) -> Result<bool> {
    let req = self.fastly_request(Request::new(
      Method::GET,
      format!("https://api.fastly.com/service/{}/version/1", service_id),
    ))?;
    let mut resp = req.send(API_BACKEND)?;
    match resp.get_status() {
      StatusCode::OK => Ok(resp.take_body_json::<FastlyServiceStatusResponse>()?.active),
      _ => bail!("Unable to authenticate with Fastly")
    }
  }
}

#[derive(Serialize)]
pub struct FastlyServiceRequest {
  #[serde(rename = "type")]
  service_type: String,
  name: String,
}

#[derive(Deserialize)]
pub struct FastlyService {
  pub id: String,
  pub domain: Option<String>,
}

#[derive(Deserialize)]
pub struct FastlyServiceStatusResponse {
  pub active: bool
}

#[derive(Serialize, Deserialize)]
pub struct FastlyDomain {
  pub name: String,
}

#[derive(Serialize, Deserialize)]
pub struct FastlyDictionary {
  pub id: Option<String>,
  pub name: String,
}

#[derive(Serialize)]
pub struct FastlyDictionaryUpdateRequest {
  pub items: Vec<FastlyDictionaryItemAction>,
}

#[derive(Serialize)]
pub struct FastlyDictionaryItemAction {
  pub op: String,
  pub item_key: String,
  pub item_value: String,
}

#[derive(Serialize)]
pub struct FastlyBackend {
  pub name: String,
  pub address: String,
  pub port: i32,
}

#[derive(Deserialize, Serialize)]
pub struct FastlyUser {
  pub name: String,
  pub customer_id: String,
}

#[derive(Deserialize)]
pub struct AuthParams {
  pub token: String,
}
