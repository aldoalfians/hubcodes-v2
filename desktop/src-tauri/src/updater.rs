//! App updates from the GitHub release feed.

use super::*;

const RELEASE_REPO: &str = "HubStudio-id/hubcodes";

/// A GitHub token for the release repo while it is private: `GH_TOKEN`,
/// else what a logged-in `gh` holds. None is fine for a public repo.
pub(crate) fn gh_token() -> Option<String> {
    if let Some(t) = std::env::var("GH_TOKEN").ok().or_else(|| std::env::var("GITHUB_TOKEN").ok()).filter(|t| !t.trim().is_empty()) {
        return Some(t.trim().to_string());
    }
    let home = dirs::home_dir().unwrap_or_default();
    let path = format!("{}/.local/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin", home.display());
    let out = std::process::Command::new("gh").env("PATH", path).args(["auth", "token"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let t = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if t.is_empty() { None } else { Some(t) }
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateInfo {
    version: String,
    notes: String,
    date: Option<String>,
    current: String,
}

/// Resolve the release feed. GitHub's `releases/latest/download/…` URL is
/// unauthenticated, so for a private repo the feed is read through the API:
/// the `latest.json` asset of the latest release, requested as a raw file.
/// The same headers go on the tarball download, whose URL the release script
/// writes in the API form too.
pub(crate) fn updater_for(app: &AppHandle) -> Result<tauri_plugin_updater::Updater, String> {
    use tauri_plugin_updater::UpdaterExt;
    let token = gh_token();
    let body = hubcodes_core::update::github_get(&format!("https://api.github.com/repos/{RELEASE_REPO}/releases/latest"), token.as_deref(), 20).map_err(|e| format!("could not read the latest release of {RELEASE_REPO}: {e:#}"))?;
    let rel: serde_json::Value = serde_json::from_slice(&body).map_err(err)?;
    let feed = rel["assets"].as_array().and_then(|a| a.iter().find(|x| x["name"] == "latest.json")).and_then(|x| x["url"].as_str()).ok_or("the latest release has no latest.json asset")?;
    let mut b = app.updater_builder().endpoints(vec![feed.parse().map_err(err)?]).map_err(err)?.header("Accept", "application/octet-stream").map_err(err)?;
    if let Some(t) = token {
        b = b.header("Authorization", format!("Bearer {t}")).map_err(err)?;
    }
    b.build().map_err(err)
}

/// Is a newer release out? Checked on launch and every few hours.
#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    let current = app.package_info().version.to_string();
    let updater = match updater_for(&app) {
        Ok(u) => u,
        Err(e) => {
            log_line(&format!("update check: {e}"));
            return Err(e);
        }
    };
    let u = updater.check().await.map_err(|e| {
        log_line(&format!("update check failed: {e}"));
        err(e)
    })?;
    log_line(&match &u {
        Some(u) => format!("update check: {} available (running {current})", u.version),
        None => format!("update check: {current} is current"),
    });
    Ok(u.map(|u| UpdateInfo { version: u.version.clone(), notes: u.body.clone().unwrap_or_default(), date: u.date.map(|d| d.to_string()), current: current.clone() }))
}

/// Download, verify (minisign, pubkey in tauri.conf.json), install, restart.
/// Progress goes out as `update-progress` {downloaded, total}.
#[tauri::command]
pub async fn update_install(app: AppHandle) -> Result<(), String> {
    let updater = updater_for(&app)?;
    let Some(u) = updater.check().await.map_err(err)? else { return Err("already on the latest version".into()) };
    log_line(&format!("updating to {}", u.version));
    let mut done: u64 = 0;
    let progress = app.clone();
    u.download_and_install(
        move |chunk, total| {
            done += chunk as u64;
            let _ = progress.emit("update-progress", serde_json::json!({ "downloaded": done, "total": total }));
        },
        || {},
    )
    .await
    .map_err(err)?;
    log_line("update installed; restarting");
    app.restart();
}
