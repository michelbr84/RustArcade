//! `rustarcade doctor`: environment diagnostics with repair guidance.

use serde::Serialize;

use crate::paths::check_writable;
use crate::platform::TerminalInfo;
use crate::services::{GameState, Services};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum CheckStatus {
    Ok,
    Warning,
    Error,
}

impl CheckStatus {
    pub fn label(self) -> &'static str {
        match self {
            CheckStatus::Ok => "OK",
            CheckStatus::Warning => "WARNING",
            CheckStatus::Error => "ERROR",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckResult {
    pub name: String,
    pub status: CheckStatus,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DoctorReport {
    pub checks: Vec<CheckResult>,
}

impl DoctorReport {
    fn push(
        &mut self,
        name: &str,
        status: CheckStatus,
        detail: impl Into<String>,
        fix: Option<String>,
    ) {
        self.checks.push(CheckResult {
            name: name.to_string(),
            status,
            detail: detail.into(),
            fix,
        });
    }

    pub fn worst(&self) -> CheckStatus {
        self.checks
            .iter()
            .map(|c| c.status)
            .max()
            .unwrap_or(CheckStatus::Ok)
    }

    /// Process exit code: errors → 1, otherwise 0.
    pub fn exit_code(&self) -> i32 {
        if self.worst() == CheckStatus::Error {
            1
        } else {
            0
        }
    }

    pub fn count(&self, status: CheckStatus) -> usize {
        self.checks.iter().filter(|c| c.status == status).count()
    }
}

/// Run every diagnostic.
pub async fn run(services: &Services) -> DoctorReport {
    let mut report = DoctorReport::default();
    let platform = services.platform();
    report.push(
        "Operating system",
        CheckStatus::Ok,
        platform.os.label(),
        None,
    );
    report.push(
        "Architecture",
        CheckStatus::Ok,
        platform.arch.as_str(),
        None,
    );

    let term = TerminalInfo::detect();
    let term_detail = match (&term.term, term.size) {
        (Some(t), Some((c, r))) => format!("{t} ({c}x{r})"),
        (Some(t), None) => t.clone(),
        (None, _) => "TERM is not set".into(),
    };
    let term_status = if !term.is_tty || term.size.is_some_and(|(c, r)| c < 60 || r < 16) {
        CheckStatus::Warning
    } else {
        CheckStatus::Ok
    };
    report.push(
        "Terminal",
        term_status,
        if term.is_tty {
            term_detail
        } else {
            format!("{term_detail}; stdout is not a terminal")
        },
        (term_status != CheckStatus::Ok).then(|| {
            "Run RustArcade in an interactive terminal of at least 60x16 characters.".to_string()
        }),
    );

    for (name, dir) in services.paths().describe() {
        match check_writable(&dir) {
            Ok(()) => report.push(
                &format!("Directory ({name})"),
                CheckStatus::Ok,
                dir.display().to_string(),
                None,
            ),
            Err(e) => report.push(
                &format!("Directory ({name})"),
                CheckStatus::Error,
                e.to_string(),
                Some(format!(
                    "Make {} writable or set RUSTARCADE_HOME to another location.",
                    dir.display()
                )),
            ),
        }
    }

    let tools = services.tools();
    for (tool, purpose) in [
        (&tools.cargo, "building games from source"),
        (&tools.rustc, "compiling Rust code"),
        (&tools.git, "cloning game repositories"),
    ] {
        match &tool.path {
            Some(path) => report.push(
                &capitalize(&tool.name),
                CheckStatus::Ok,
                format!("{} ({})", tool.version.clone().unwrap_or_else(|| "version unknown".into()), path.display()),
                None,
            ),
            None => report.push(
                &capitalize(&tool.name),
                CheckStatus::Warning,
                format!("not found — needed for {purpose}"),
                Some(if tool.name == "git" {
                    "Install Git from https://git-scm.com/downloads.".into()
                } else {
                    "Install the Rust toolchain from https://rustup.rs (prebuilt GitHub releases work without it).".into()
                }),
            ),
        }
    }

    if services.offline() {
        report.push(
            "Network",
            CheckStatus::Warning,
            "offline mode enabled",
            Some("Unset RUSTARCADE_OFFLINE to enable downloads.".into()),
        );
    } else {
        let url = format!("{}/rate_limit", services.endpoints().github_api);
        match services.http().get_github_json::<serde_json::Value>(&url, None).await {
            Ok(r) => {
                let remaining = r.rate_limit_remaining.map(|n| format!("GitHub API reachable, {n} requests remaining")).unwrap_or_else(|| "GitHub API reachable".into());
                let status = if r.rate_limit_remaining == Some(0) { CheckStatus::Warning } else { CheckStatus::Ok };
                report.push("Network", status, remaining, (status != CheckStatus::Ok).then(|| "Set GITHUB_TOKEN to raise the API limit.".into()));
            }
            Err(crate::error::NetworkError::NotFound { .. }) | Err(crate::error::NetworkError::Status { .. }) => {
                report.push("Network", CheckStatus::Ok, "GitHub API reachable", None);
            }
            Err(crate::error::NetworkError::RateLimited { .. }) => {
                report.push("Network", CheckStatus::Warning, "GitHub API rate limit exceeded", Some("Set GITHUB_TOKEN or wait for the limit to reset.".into()));
            }
            Err(e) => report.push(
                "Network",
                CheckStatus::Warning,
                format!("GitHub API unreachable: {e}"),
                Some("Check your internet connection or proxy settings. Installed games keep working offline.".into()),
            ),
        }
    }

    let builtin = crate::catalog::load_builtin();
    if builtin.is_clean() {
        report.push(
            "Built-in catalog",
            CheckStatus::Ok,
            format!("{} games", builtin.ok.len()),
            None,
        );
    } else {
        report.push(
            "Built-in catalog",
            CheckStatus::Error,
            format!("{} invalid manifest(s)", builtin.errors.len()),
            Some("Reinstall RustArcade; run `rustarcade catalog validate` for details.".into()),
        );
    }
    let status = services.catalog_status();
    let cached_status = match &status {
        crate::services::CatalogStatus::Failed { reason, .. } => {
            (CheckStatus::Warning, reason.clone())
        }
        other => (CheckStatus::Ok, other.label()),
    };
    report.push(
        "Remote catalog",
        cached_status.0,
        cached_status.1,
        (cached_status.0 != CheckStatus::Ok)
            .then(|| "Run `rustarcade catalog update` when online.".into()),
    );

    let installed = services.installed();
    let broken: Vec<String> = installed
        .iter()
        .filter_map(|v| match &v.state {
            GameState::Broken(reason) => Some(format!("{} ({reason})", v.manifest.id)),
            _ => None,
        })
        .collect();
    if broken.is_empty() {
        report.push(
            "Installation registry",
            CheckStatus::Ok,
            format!("{} installed game(s)", installed.len()),
            None,
        );
    } else {
        report.push(
            "Installation registry",
            CheckStatus::Warning,
            format!("{} broken: {}", broken.len(), broken.join(", ")),
            Some("Reinstall the affected games from their details screen or with `rustarcade install <id>`.".into()),
        );
    }
    for note in services.startup_notes() {
        report.push("Startup", CheckStatus::Warning, note.clone(), None);
    }
    report
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::Endpoints;
    use crate::platform::Tools;
    use crate::services::OpenOptions;

    #[tokio::test]
    async fn doctor_reports_sections() {
        let root = tempfile::tempdir().unwrap();
        let s = Services::open(OpenOptions {
            root: Some(root.path().to_path_buf()),
            tools: Some(Tools::none()),
            endpoints: Some(Endpoints::default()),
            allow_insecure_local: Some(true),
            offline: true,
            ..OpenOptions::default()
        })
        .unwrap();
        let report = run(&s).await;
        let names: Vec<&str> = report.checks.iter().map(|c| c.name.as_str()).collect();
        for expected in [
            "Operating system",
            "Architecture",
            "Terminal",
            "Cargo",
            "Git",
            "Network",
            "Built-in catalog",
            "Installation registry",
        ] {
            assert!(names.contains(&expected), "missing {expected}: {names:?}");
        }
        assert_eq!(report.worst(), CheckStatus::Warning);
        assert_eq!(report.exit_code(), 0);
        assert!(report.count(CheckStatus::Warning) >= 3);
        assert_eq!(capitalize("cargo"), "Cargo");
    }
}
