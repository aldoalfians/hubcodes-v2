//! Tokens and API keys in the system's secret store instead of
//! `hubcodes.toml`: the Keychain on macOS, the login keyring (Secret
//! Service: GNOME Keyring, KWallet) on Linux through `secret-tool`.
//! The file holds a reference, `${keychain:NAME}`, which [`crate::mcp::expand_env`]
//! resolves where the value is used, the same way it resolves `${ENV_VAR}`.
//!
//! Items go through `/usr/bin/security` (service `hubcodes`, account NAME)
//! for both writing and reading, so the app, the CLI and the daemon all read
//! them without a Keychain prompt: the item's access list names `security`.
//! The value is written over stdin (`security -i`), never on a command line
//! where `ps` would show it. On Linux `secret-tool store` reads it from
//! stdin too (Debian/Ubuntu: `libsecret-tools`, Fedora: `libsecret`).

use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const SERVICE: &str = "hubcodes";
/// Reads are cached this long, so a poll loop does not run `security` each pass.
const FRESH: Duration = Duration::from_secs(60);

/// What goes in the config for a secret named `name`.
pub fn reference(name: &str) -> String {
    format!("${{keychain:{name}}}")
}

/// The name in a `${keychain:NAME}` reference, if `value` is one.
pub fn name_of(value: &str) -> Option<&str> {
    value.trim().strip_prefix("${keychain:").and_then(|r| r.strip_suffix('}')).filter(|n| valid(n))
}

/// A value that is a plain secret: not empty and not a `${…}` reference.
pub fn is_raw(value: &str) -> bool {
    let v = value.trim();
    // "twg" is no secret: it says to sign in through the twg CLI.
    !v.is_empty() && !(v.starts_with("${") && v.ends_with('}')) && !crate::twg::is_twg(v)
}

fn valid(name: &str) -> bool {
    !name.is_empty() && name.len() <= 120 && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ':' | '@'))
}

/// What the store is called here, for messages.
pub fn store_name() -> &'static str {
    if cfg!(target_os = "macos") { "Keychain" } else { "keyring" }
}

/// Linux: `secret-tool <args>` for item `name`, the value on stdin when given.
#[cfg(not(target_os = "macos"))]
fn secret_tool(args: &[&str], name: &str, stdin: Option<&str>) -> Result<std::process::Output> {
    let mut c = Command::new("secret-tool");
    c.args(args).args(["service", SERVICE, "account", name]).stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = c.spawn().map_err(|e| anyhow!("secret-tool is not installed ({e}): sudo apt install libsecret-tools, or sudo dnf install libsecret; or use ${{ENV_VAR}}"))?;
    if let Some(v) = stdin {
        child.stdin.take().context("secret-tool stdin")?.write_all(v.as_bytes())?;
    }
    // A locked keyring can wait for an unlock prompt nobody sees (the
    // daemon): give up rather than hang the tender.
    let started = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            return Ok(child.wait_with_output()?);
        }
        if started.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(anyhow!("the keyring did not answer in 10 s (locked? unlock it, or use ${{ENV_VAR}})"));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A keychain other than the login one (tests use a throwaway keychain).
#[cfg(target_os = "macos")]
fn keychain() -> Option<String> {
    std::env::var("HUBCODES_KEYCHAIN").ok().filter(|k| !k.is_empty())
}

/// name → (when read, value)
type Cache = Mutex<HashMap<String, (Instant, Option<String>)>>;

fn cache() -> &'static Cache {
    static C: OnceLock<Cache> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The secret, or `None` when there is no such item (or no Keychain here).
pub fn get(name: &str) -> Option<String> {
    if !valid(name) {
        return None;
    }
    if let Some((at, v)) = cache().lock().unwrap_or_else(|e| e.into_inner()).get(name) {
        if at.elapsed() < FRESH {
            return v.clone();
        }
    }
    let v = read(name);
    cache().lock().unwrap_or_else(|e| e.into_inner()).insert(name.to_string(), (Instant::now(), v.clone()));
    v
}

#[cfg(target_os = "macos")]
fn read(name: &str) -> Option<String> {
    let mut c = Command::new("/usr/bin/security");
    c.args(["find-generic-password", "-s", SERVICE, "-a", name, "-w"]);
    if let Some(k) = keychain() {
        c.arg(k);
    }
    c.stderr(Stdio::null()).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim_end_matches(['\n', '\r']).to_string())
}

#[cfg(not(target_os = "macos"))]
fn read(name: &str) -> Option<String> {
    secret_tool(&["lookup"], name, None).ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim_end_matches(['\n', '\r']).to_string()).filter(|v| !v.is_empty())
}

/// Store (or replace) a secret. Returns the reference to put in the config.
pub fn set(name: &str, value: &str) -> Result<String> {
    if !valid(name) {
        return Err(anyhow!("'{name}' is not a usable secret name (letters, digits, . _ - : @)"));
    }
    let value = value.trim();
    if value.is_empty() {
        return Err(anyhow!("nothing to store"));
    }
    if value.contains(['\n', '\r']) {
        return Err(anyhow!("a token is one line"));
    }
    write(name, value)?;
    cache().lock().unwrap_or_else(|e| e.into_inner()).remove(name);
    match get(name) {
        Some(v) if v == value => Ok(reference(name)),
        _ => Err(anyhow!("the {} did not keep the secret (is it locked?)", store_name())),
    }
}

#[cfg(not(target_os = "macos"))]
fn write(name: &str, value: &str) -> Result<()> {
    let out = secret_tool(&["store", &format!("--label=hubcodes: {name}")], name, Some(value))?;
    if !out.status.success() {
        return Err(anyhow!("keyring: {} (is a keyring such as GNOME Keyring running and unlocked?)", String::from_utf8_lossy(&out.stderr).trim().lines().last().unwrap_or("could not store the secret")));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn write(name: &str, value: &str) -> Result<()> {
    let quote = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
    let mut line = format!("add-generic-password -U -s {SERVICE} -a {} -l {} -w {}", quote(name), quote(&format!("hubcodes: {name}")), quote(value));
    if let Some(k) = keychain() {
        line.push(' ');
        line.push_str(&quote(&k));
    }
    line.push('\n');
    let mut child = Command::new("/usr/bin/security").arg("-i").stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().context("run security")?;
    child.stdin.take().context("security stdin")?.write_all(line.as_bytes())?;
    let out = child.wait_with_output()?;
    let err = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() || err.contains("error") {
        return Err(anyhow!("Keychain: {}", err.trim().lines().last().unwrap_or("could not store the secret")));
    }
    Ok(())
}

/// Remove a secret; missing is fine.
pub fn delete(name: &str) -> Result<()> {
    if !valid(name) {
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        let mut c = Command::new("/usr/bin/security");
        c.args(["delete-generic-password", "-s", SERVICE, "-a", name]);
        if let Some(k) = keychain() {
            c.arg(k);
        }
        let _ = c.stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    #[cfg(not(target_os = "macos"))]
    let _ = secret_tool(&["clear"], name, None);
    cache().lock().unwrap_or_else(|e| e.into_inner()).remove(name);
    Ok(())
}

/// Move a plain secret in `field` to the Keychain under `name` and leave
/// the reference. `Ok(true)` when it moved; a value that already refers to
/// one (`${…}` anywhere in it) stays as it is.
pub fn stash(field: &mut String, name: &str) -> Result<bool> {
    if !is_raw(field) || field.contains("${") {
        return Ok(false);
    }
    *field = set(name, field)?;
    Ok(true)
}

/// Headers that carry a credential (Authorization, X-API-Key, …).
pub fn is_secret_header(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n == "authorization" || n.contains("token") || n.contains("key") || n.contains("secret") || n.contains("cookie")
}

/// Stash the credential headers of `headers` as `<prefix>:header:<Name>`.
pub fn stash_headers(headers: &mut std::collections::BTreeMap<String, String>, prefix: &str) -> Result<usize> {
    let mut n = 0;
    for (k, v) in headers.iter_mut() {
        if is_secret_header(k) && stash(v, &format!("{prefix}:header:{}", k.to_ascii_lowercase()))? {
            n += 1;
        }
    }
    Ok(n)
}

/// A name for a secret from what it belongs to: `jira-token@hubstudio.atlassian.net`.
pub fn name_for(what: &str, place: &str) -> String {
    let clean = |s: &str| s.trim().trim_start_matches("https://").trim_start_matches("http://").trim_end_matches('/').chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '-' }).collect::<String>();
    let place = clean(place);
    let what = clean(what);
    if place.is_empty() {
        what
    } else {
        format!("{what}@{place}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_and_raw_values() {
        assert_eq!(reference("jira@x"), "${keychain:jira@x}");
        assert_eq!(name_of("${keychain:jira@x}"), Some("jira@x"));
        assert_eq!(name_of("${JIRA_TOKEN}"), None);
        assert!(is_raw("ATATT3x"));
        assert!(!is_raw("${JIRA_TOKEN}") && !is_raw("${keychain:a}") && !is_raw(" ") && !is_raw("twg"));
        assert_eq!(name_for("jira-token", "https://hubstudio.atlassian.net/"), "jira-token@hubstudio.atlassian.net");
        assert!(set("bad name", "x").is_err());
    }

    /// Writes to a throwaway keychain, never the login one.
    #[test]
    #[cfg(target_os = "macos")]
    fn a_secret_round_trips_through_a_throwaway_keychain() {
        let dir = std::env::temp_dir().join(format!("hubcodes-kc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let kc = dir.join("t.keychain-db");
        let kcs = kc.display().to_string();
        let ok = Command::new("/usr/bin/security").args(["create-keychain", "-p", "pw", &kcs]).status().map(|s| s.success()).unwrap_or(false)
            && Command::new("/usr/bin/security").args(["unlock-keychain", "-p", "pw", &kcs]).status().map(|s| s.success()).unwrap_or(false);
        if !ok {
            eprintln!("skipping: cannot create a keychain here");
            return;
        }
        std::env::set_var("HUBCODES_KEYCHAIN", &kcs);
        let name = format!("test-{}", std::process::id());
        let tricky = r#"ab"c d$e\f"#;
        assert_eq!(set(&name, tricky).unwrap(), reference(&name));
        assert_eq!(get(&name).as_deref(), Some(tricky));
        assert_eq!(crate::mcp::expand_env(&format!("Bearer {}", reference(&name))), format!("Bearer {tricky}"));
        set(&name, "second").unwrap();
        assert_eq!(get(&name).as_deref(), Some("second"));
        delete(&name).unwrap();
        assert_eq!(get(&name), None);
        // Saving settings moves plain tokens out of the file.
        let cfg = dir.join("hubcodes.toml");
        std::fs::write(&cfg, format!("[[tracker]]\nname = \"j{n}\"\nkind = \"jira\"\ntoken = \"plain-token\"\nemail = \"a@b.c\"\n[[tracker.headers]]\n\n[[connection]]\nname = \"c{n}\"\nkind = \"plane\"\ntoken = \"${{PLANE_KEY}}\"\n", n = std::process::id()).replace("[[tracker.headers]]\n\n", "")).unwrap();
        let mut reg = crate::providers::Registry::from_file(&cfg).unwrap();
        let moved = reg.stash_secrets().unwrap();
        assert_eq!(moved, vec![format!("tracker j{}", std::process::id())]);
        let tok = reg.trackers[0].token.clone();
        assert_eq!(name_of(&tok), Some(format!("tracker:j{}:token", std::process::id()).as_str()));
        assert_eq!(crate::mcp::expand_env(&tok), "plain-token");
        assert_eq!(reg.trackers[0].email, "a@b.c", "an email is not a secret");
        assert_eq!(reg.connections[0].token, "${PLANE_KEY}", "a reference stays");
        assert!(reg.stash_secrets().unwrap().is_empty(), "a second save moves nothing");
        delete(name_of(&tok).unwrap()).unwrap();
        std::env::remove_var("HUBCODES_KEYCHAIN");
        let _ = Command::new("/usr/bin/security").args(["delete-keychain", &kcs]).status();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
