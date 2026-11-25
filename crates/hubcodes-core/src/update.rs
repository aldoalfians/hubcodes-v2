//! Is this hubcodes the newest release, and how to install the newest one.
//! `hubcodes update`, `hubcodes update --check` and the doctor's Version
//! line use this; the desktop app also updates itself through its feed.

use anyhow::{anyhow, Context, Result};
use std::process::{Command, Stdio};

/// The repository releases come from.
pub const REPO: &str = "HubStudio-id/hubcodes";

/// This build's version.
pub fn current() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// A GitHub token for the private repository: `GH_TOKEN`, `GITHUB_TOKEN`,
/// else the logged-in `gh`.
pub fn token() -> Option<String> {
    if let Some(t) = std::env::var("GH_TOKEN").ok().or_else(|| std::env::var("GITHUB_TOKEN").ok()).filter(|t| !t.trim().is_empty()) {
        return Some(t.trim().to_string());
    }
    let out = Command::new("gh").args(["auth", "token"]).stderr(Stdio::null()).output().ok()?;
    let t = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !t.is_empty()).then_some(t)
}

/// The release asset this platform installs from.
pub fn platform_asset() -> String {
    if cfg!(target_os = "macos") {
        "hubcodes.app.tar.gz".into()
    } else {
        format!("hubcodes-cli-linux-{}.tar.gz", if cfg!(target_arch = "aarch64") { "aarch64" } else { "x86_64" })
    }
}

/// The newest published release that has this platform's build (`0.1.31`),
/// asked with a short timeout. Linux builds are attached some minutes after
/// the Mac release, so meanwhile Linux sees the release before it.
pub fn latest() -> Result<String> {
    let tok = token();
    let body = github_get(&format!("https://api.github.com/repos/{REPO}/releases?per_page=15"), tok.as_deref(), 8)?;
    let v: serde_json::Value = serde_json::from_slice(&body).context("releases JSON")?;
    newest_with(&v, &platform_asset()).ok_or_else(|| anyhow!("no release of {REPO} has {} yet", platform_asset()))
}

/// GET a GitHub API address with curl. A failure says the HTTP status and
/// what it most likely means, so a person can fix it without a log.
pub fn github_get(url: &str, token: Option<&str>, timeout_secs: u32) -> Result<Vec<u8>> {
    let mut c = Command::new("curl");
    c.args(["-sS", "--max-time", &timeout_secs.to_string(), "-w", "\n%{http_code}", "-H", "Accept: application/vnd.github+json", "-H", "User-Agent: hubcodes"]);
    if let Some(t) = token {
        c.args(["-H", &format!("Authorization: Bearer {t}")]);
    }
    let out = c.arg(url).output().context("run curl (is curl installed?)")?;
    let text = String::from_utf8_lossy(&out.stdout);
    let (body, code) = text.rsplit_once('\n').map(|(b, c)| (b, c.trim().parse::<u16>().unwrap_or(0))).unwrap_or((&text, 0));
    if out.status.success() && (200..300).contains(&code) {
        return Ok(body.as_bytes().to_vec());
    }
    let said = serde_json::from_str::<serde_json::Value>(body).ok().and_then(|v| v["message"].as_str().map(String::from)).unwrap_or_default();
    Err(anyhow!("{}", explain(code, token.is_some(), if code == 0 { String::from_utf8_lossy(&out.stderr).trim().to_string() } else { said })))
}

/// Why GitHub said no, in words a person can act on.
pub fn explain(code: u16, signed_in: bool, detail: String) -> String {
    let why = match code {
        0 => "GitHub did not answer (offline, a proxy, or a firewall)".to_string(),
        401 => "HTTP 401: GitHub refused the token (expired or revoked: run gh auth login again, or replace GH_TOKEN)".to_string(),
        403 => "HTTP 403: rate-limited, or the token may not read this repository (run gh auth login again)".to_string(),
        404 if !signed_in => format!("HTTP 404: {REPO} is private and nothing here is signed in to GitHub (run gh auth login, or set GH_TOKEN)"),
        404 => format!("HTTP 404: the GitHub account signed in here cannot see {REPO} (check gh auth status and GH_TOKEN, or ask an admin of the repository for access)"),
        c => format!("HTTP {c} from GitHub"),
    };
    if detail.is_empty() { why } else { format!("{why}: {detail}") }
}

/// The newest published (not draft, not pre-release) release in a GitHub
/// releases list that carries `asset`, without the `v`.
pub fn newest_with(releases: &serde_json::Value, asset: &str) -> Option<String> {
    releases.as_array()?.iter().find_map(|r| {
        let published = !r["draft"].as_bool().unwrap_or(false) && !r["prerelease"].as_bool().unwrap_or(false);
        let has = r["assets"].as_array().map(|a| a.iter().any(|x| x["name"] == asset)).unwrap_or(false);
        (published && has).then(|| r["tag_name"].as_str().unwrap_or("").trim_start_matches('v').to_string())
    })
}

/// `a` is a newer version than `b` (numeric, major.minor.patch; a
/// pre-release suffix such as `-rc.1` sorts before the release).
pub fn newer(a: &str, b: &str) -> bool {
    key(a) > key(b)
}

fn key(v: &str) -> (Vec<u64>, bool) {
    let v = v.trim().trim_start_matches('v');
    let (main, pre) = v.split_once('-').map(|(m, p)| (m, !p.is_empty())).unwrap_or((v, false));
    let mut nums: Vec<u64> = main.split('.').map(|p| p.parse().unwrap_or(0)).collect();
    nums.resize(3, 0);
    (nums, !pre)
}

/// What this machine runs to install or update hubcodes.
pub fn install_command() -> String {
    format!("gh api -H \"Accept: application/vnd.github.raw\" repos/{REPO}/contents/install.sh | sh")
}

/// Where this hubcodes came from: the macOS app, a Linux AppImage-less CLI, or a build tree.
pub fn install_kind() -> &'static str {
    let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
    if exe.contains("/target/release/") || exe.contains("/target/debug/") {
        "build"
    } else if exe.contains(".app/Contents/MacOS/") {
        "app"
    } else {
        "cli"
    }
}

/// Run the installer for the newest release (replaces the app and the CLI).
pub fn run_installer() -> Result<()> {
    let cmd = if Command::new("gh").args(["auth", "status"]).stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false) {
        install_command()
    } else {
        let t = token().ok_or_else(|| anyhow!("sign in first: gh auth login (or set GH_TOKEN)"))?;
        std::env::set_var("GH_TOKEN", &t);
        format!("curl -fsSL -H \"Authorization: Bearer $GH_TOKEN\" -H \"Accept: application/vnd.github.raw\" https://api.github.com/repos/{REPO}/contents/install.sh | sh")
    };
    let st = Command::new("sh").args(["-c", &cmd]).status().context("run the installer")?;
    if !st.success() {
        return Err(anyhow!("the installer failed (exit {})", st.code().unwrap_or(-1)));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_say_the_status_and_what_to_do() {
        assert!(explain(404, false, String::new()).contains("gh auth login"));
        assert!(explain(404, true, "Not Found".into()).contains("cannot see") && explain(404, true, "Not Found".into()).ends_with(": Not Found"));
        assert!(explain(401, true, String::new()).starts_with("HTTP 401"));
        assert!(explain(0, true, "Could not resolve host".into()).contains("did not answer"));
        assert_eq!(explain(500, true, String::new()), "HTTP 500 from GitHub");
    }

    #[test]
    fn latest_is_the_newest_release_with_this_platforms_build() {
        let v = serde_json::json!([
            { "tag_name": "v0.1.32", "draft": true, "prerelease": false, "assets": [{ "name": "hubcodes-cli-linux-x86_64.tar.gz" }] },
            { "tag_name": "v0.1.31", "draft": false, "prerelease": false, "assets": [{ "name": "hubcodes.app.tar.gz" }] },
            { "tag_name": "v0.1.30", "draft": false, "prerelease": false, "assets": [{ "name": "hubcodes.app.tar.gz" }, { "name": "hubcodes-cli-linux-x86_64.tar.gz" }] }
        ]);
        assert_eq!(newest_with(&v, "hubcodes.app.tar.gz").as_deref(), Some("0.1.31"));
        assert_eq!(newest_with(&v, "hubcodes-cli-linux-x86_64.tar.gz").as_deref(), Some("0.1.30"), "Linux waits for CI; drafts never count");
        assert_eq!(newest_with(&v, "hubcodes-cli-linux-aarch64.tar.gz"), None);
    }

    #[test]
    fn patch_minor_and_major_releases_are_newer() {
        assert!(newer("0.1.30", "0.1.29"));
        assert!(newer("0.1.10", "0.1.9"), "numeric, not text");
        assert!(newer("0.2.0", "0.1.99"));
        assert!(newer("1.0.0", "0.9.9"));
        assert!(newer("v0.1.31", "0.1.30"));
        assert!(!newer("0.1.29", "0.1.29"));
        assert!(!newer("0.1.28", "0.1.29"));
        assert!(newer("0.1.30", "0.1.30-rc.1"), "a release is newer than its pre-release");
    }
}
