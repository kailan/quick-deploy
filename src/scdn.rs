use crate::config::BackendSpec;
use fastly::{
  http::{header, Method},
  Request,
};
use serde::{Deserialize, Serialize};
use crate::config::DeployConfig;
use anyhow::{Result, bail};

const USER_AGENT: &str = "Quick Deploy (@kailan)";
const API_BACKEND: &str = "api.fastly.com";

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

  pub fn fastly_request(&self, req: Request) -> Request {
    req
      .with_header(header::USER_AGENT, USER_AGENT)
      .with_header(header::HOST, "api.fastly.com")
      .with_header(header::ACCEPT, "application/json")
      .with_header("Fastly-Key", self.token.as_ref().unwrap())
      .with_pass(true)
  }

  pub fn fetch_user(&self) -> Result<Option<FastlyUser>> {
    if self.token == None {
      return Ok(None);
    }

    let req = self.fastly_request(Request::new(
      Method::GET,
      "https://api.fastly.com/current_user",
    ));
    let mut resp = req.send(API_BACKEND)?;
    match resp.take_body_json::<FastlyUser>() {
      Ok(user) => Ok(Some(user)),
      Err(err) => bail!(err),
    }
  }

  pub fn create_service(&self, name: &str, mut deploy: DeployConfig) -> Result<FastlyService> {
    if self.token == None {
      bail!("No Fastly API token set");
    }

    let domain = format!("{}-deploy-demo.edgecompute.app", name);

    // Create a service
    let servreq = FastlyServiceRequest {
      service_type: "wasm".to_string(),
      name: "via Quick Deploy".to_string(),
    };

    let req = self
      .fastly_request(Request::new(Method::POST, "https://api.fastly.com/service"))
      .with_pass(true)
      .with_body_json(&servreq)?;
    let mut resp = req.send(API_BACKEND)?;

    let mut service = match resp.take_body_json::<FastlyService>() {
      Ok(service) => service,
      Err(err) => bail!("Error while creating service: {}", err),
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
      ))
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
        host: "127.0.0.1".to_string(),
        port: 80
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
        ))
        .with_pass(true)
        .with_body_json(&FastlyBackend {
          name: backend.name.to_owned(),
          address: backend.host,
          port: backend.port
        }) {
        Ok(req) => req,
        Err(err) => bail!("Error while creating backend {}: {}", backend.name, err),
      };
      let mut resp = req.send(API_BACKEND)?;
      println!("{}", resp.take_body_str());
      println!("Created backend {}", backend.name);
    }

    Ok(service)
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

#[derive(Serialize, Deserialize)]
pub struct FastlyDomain {
  pub name: String,
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
