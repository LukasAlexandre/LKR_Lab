use crate::{commands, HubResult};
use serde::Serialize;
use std::path::Path;
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostingState {
    pub provider: String,
    pub authenticated: bool,
    pub pull_requests: serde_json::Value,
    pub issues: serde_json::Value,
}
pub trait GitHostingProvider {
    fn inspect(&self, path: &Path) -> HubResult<HostingState>;
}
pub struct GitHubProvider;
impl GitHostingProvider for GitHubProvider {
    fn inspect(&self, path: &Path) -> HubResult<HostingState> {
        commands::run("gh", &["auth", "status"], None)?;
        // Never read or return auth tokens. All repository reads are explicit user refreshes.
        let prs = commands::run(
            "gh",
            &[
                "pr",
                "list",
                "--limit",
                "10",
                "--state",
                "open",
                "--json",
                "number,title,url,headRefName,reviewDecision,mergeable,statusCheckRollup",
            ],
            Some(path),
        )?;
        let issues = commands::run(
            "gh",
            &[
                "issue",
                "list",
                "--limit",
                "10",
                "--state",
                "open",
                "--json",
                "number,title,url",
            ],
            Some(path),
        )?;
        Ok(HostingState {
            provider: "GitHub".into(),
            authenticated: true,
            pull_requests: serde_json::from_str(&prs).map_err(|e| e.to_string())?,
            issues: serde_json::from_str(&issues).map_err(|e| e.to_string())?,
        })
    }
}
