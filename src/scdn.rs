use fastly::{
  http::{header, Method},
  Request,
};
use serde::{Deserialize, Serialize};

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

  pub fn fetch_user(&self) -> Option<FastlyUser> {
    if self.token == None {
      return None;
    }

    let req = self.fastly_request(Request::new(
      Method::GET,
      "https://api.fastly.com/current_user",
    ));
    let mut resp = req.send(API_BACKEND).unwrap();
    match resp.take_body_json::<FastlyUser>() {
      Ok(user) => Some(user),
      Err(_) => None,
    }
  }

  pub fn create_service(&self, name: &str) -> Option<FastlyService> {
    if self.token == None {
      return None;
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
      .with_body_json(&servreq)
      .unwrap();
    let mut resp = req.send(API_BACKEND).unwrap();

    let mut service = match resp.take_body_json::<FastlyService>() {
      Ok(service) => service,
      Err(_) => return None,
    };

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
      .with_body_json(&FastlyDomain { name: domain })
      .unwrap();
    let mut resp = req.send(API_BACKEND).unwrap();

    let domain = match resp.take_body_json::<FastlyDomain>() {
      Ok(domain) => domain,
      Err(_) => return panic!("Created service but could not create domain"),
    };

    service.domain = Some(domain.name);

    // Create a backend
    let req = self
      .fastly_request(Request::new(
        Method::POST,
        format!(
          "https://api.fastly.com/service/{}/version/1/backend",
          service.id
        ),
      ))
      .with_pass(true)
      .with_body_json(&FastlyBackend {
        name: "127.0.0.1".to_string(),
        address: "127.0.0.1".to_string(),
        port: 80,
      })
      .unwrap();
    let resp = req.send(API_BACKEND).unwrap();

    Some(service)
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
  name: String,
  customer_id: String,
}

#[derive(Deserialize)]
pub struct AuthParams {
  pub token: String,
}
