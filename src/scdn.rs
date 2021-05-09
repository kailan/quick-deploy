use fastly::{Request, Dictionary, http::{Method, header}};
use serde::{Serialize, Deserialize};

const USER_AGENT: &str = "Quick Deploy (@kailan)";
const API_BACKEND: &str = "api.fastly.com";

pub struct FastlyClient {
  pub token: Option<String>
}

impl FastlyClient {
  pub fn from_token(token: String) -> FastlyClient {
    FastlyClient {
      token: Some(token)
    }
  }

  pub fn new() -> FastlyClient {
    FastlyClient {
      token: None
    }
  }

  pub fn fastly_request(&self, req: Request) -> Request {
    req.with_header(header::USER_AGENT, USER_AGENT).with_header(header::HOST, "api.fastly.com").with_header(header::ACCEPT, "application/json").with_header("Fastly-Key", self.token.as_ref().unwrap()).with_pass(true)
  }

  pub fn fetch_user(&self) -> Option<FastlyUser> {
    if self.token == None {
      return None
    }

    let req = self.fastly_request(Request::new(Method::GET, "https://api.fastly.com/current_user"));
    let mut resp = req.send(API_BACKEND).unwrap();
    match resp.take_body_json::<FastlyUser>() {
      Ok(user) => Some(user),
      Err(_) => None
    }
  }
}

#[derive(Deserialize, Serialize)]
pub struct FastlyUser {
  name: String,
  customer_id: String
}

#[derive(Deserialize)]
pub struct AuthParams {
    pub token: String
}
