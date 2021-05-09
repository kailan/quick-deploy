use serde::{Serialize, Deserialize};

impl DeployConfig {
  pub fn from_toml(config: &str) -> DeployConfig {
    toml::from_str(config).unwrap()
  }
}

#[derive(Serialize, Deserialize)]
pub struct DeployConfig {
  pub manifest_version: i32,
  #[serde(rename(deserialize = "backend"))]
  pub backends: Vec<BackendSpec>,
  #[serde(rename(deserialize = "dictionary"))]
  pub dictionaries: Vec<DictionarySpec>
}

#[derive(Serialize, Deserialize)]
pub struct BackendSpec {
  pub name: String,
  pub host: String,
  pub port: i32
}

#[derive(Serialize, Deserialize)]
pub struct DictionarySpec {
  pub name: String,
  #[serde(rename(deserialize = "key"))]
  pub keys: Vec<DictionaryKeySpec>
}

#[derive(Serialize, Deserialize)]
pub struct DictionaryKeySpec {
  pub key: String,
  #[serde(rename = "type")]
  pub input_type: String,
  pub comment: String
}