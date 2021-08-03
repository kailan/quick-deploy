use serde::{Deserialize, Serialize};
use anyhow::Result;
use crate::ActionParams;

impl DeployConfigSpec {
  pub fn from_toml(manifest: &str) -> Result<DeployConfigSpec> {
    let manifest: Manifest = toml::from_str(manifest)?;
    Ok(manifest.setup.unwrap_or(DeployConfigSpec {
      backends: vec![],
      dictionaries: vec![]
    }))
  }
}

#[derive(Deserialize)]
pub struct Manifest {
  pub setup: Option<DeployConfigSpec>
}

pub struct DeployConfig {
  pub spec: DeployConfigSpec,
  pub params: ActionParams
}

#[derive(Serialize, Deserialize)]
pub struct DeployConfigSpec {
  pub backends: Vec<BackendSpec>,
  pub dictionaries: Vec<DictionarySpec>,
}

#[derive(Serialize, Deserialize)]
pub struct BackendSpec {
  pub prompt: Option<String>,
  pub name: String,
  pub address: String,
  pub port: Option<i32>,
}

#[derive(Serialize, Deserialize)]
pub struct DictionarySpec {
  pub name: String,
  pub items: Vec<DictionaryItemSpec>,
}

#[derive(Serialize, Deserialize)]
pub struct DictionaryItemSpec {
  pub key: String,
  pub input_type: String,
  pub prompt: Option<String>,
  pub value: Option<String>
}
