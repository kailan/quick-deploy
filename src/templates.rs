use crate::github::{GitHubRepository, GitHubUser, GitHubNWO};
use crate::scdn::FastlyUser;
use crate::DeployConfigSpec;

use serde::Serialize;
use tinytemplate::TinyTemplate;

pub struct TemplateRenderer<'a> {
  tt: TinyTemplate<'a>,
}

#[derive(Serialize)]
pub struct DeployContext {
  pub src: GitHubRepository,
  pub dest_nwo: Option<GitHubNWO>,
  pub github_user: Option<GitHubUser>,
  pub fastly_user: Option<FastlyUser>,
  pub can_fork: bool,
  pub can_deploy: bool,
  pub config_spec: Option<DeployConfigSpec>,
}

#[derive(Serialize)]
pub struct ErrorContext {
  pub message: String,
}

#[derive(Serialize)]
pub struct SuccessContext {
  pub application_url: String,
  pub actions_url: String,
  pub repo_nwo: GitHubNWO,
  pub service_id: String,
  pub is_ready: bool
}

#[derive(Serialize)]
pub struct IndexContext {
  pub button_nwo: Option<String>
}

impl TemplateRenderer<'_> {
  pub fn new() -> TemplateRenderer<'static> {
    let mut tt = TinyTemplate::new();

    tt.add_template("index", include_str!("static/index.html"))
      .unwrap();
    tt.add_template("deploy", include_str!("static/deploy.html"))
      .unwrap();
    tt.add_template("error", include_str!("static/error.html"))
      .unwrap();
    tt.add_template("success", include_str!("static/success.html"))
      .unwrap();

    TemplateRenderer { tt }
  }

  pub fn render_index_page(&self, ctx: IndexContext) -> String {
    self.tt.render("index", &ctx).unwrap()
  }

  pub fn render_deploy_page(&self, ctx: DeployContext) -> String {
    self.tt.render("deploy", &ctx).unwrap()
  }

  pub fn render_error_page(&self, ctx: ErrorContext) -> String {
    self.tt.render("error", &ctx).unwrap()
  }

  pub fn render_success_page(&self, ctx: SuccessContext) -> String {
    self.tt.render("success", &ctx).unwrap()
  }
}
