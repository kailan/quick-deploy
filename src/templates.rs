use crate::scdn::FastlyUser;
use crate::github::GitHubUser;

use tinytemplate::TinyTemplate;
use serde::Serialize;

pub struct TemplateRenderer<'a> {
  tt: TinyTemplate<'a>,
}

#[derive(Serialize)]
pub struct DeployContext {
  pub src: SourceRepository,
  pub github_user: Option<GitHubUser>,
  pub fastly_user: Option<FastlyUser>,
  pub can_deploy: bool
}

#[derive(Serialize)]
pub struct SourceRepository {
  pub owner: String,
  pub name: String
}

impl TemplateRenderer<'_> {
  pub fn new() -> TemplateRenderer<'static> {
    let mut tt = TinyTemplate::new();

    tt.add_template("deploy", include_str!("static/deploy.html")).unwrap();

    TemplateRenderer { tt }
  }

  pub fn render_deploy_page(&mut self, ctx: DeployContext) -> String {
    self.tt.render("deploy", &ctx).unwrap()
  }
}
