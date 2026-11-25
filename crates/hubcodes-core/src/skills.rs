//! Agent skills (`<name>/SKILL.md` folders) across providers and devices.
//!
//! `~/.agents/skills` is the device's shared folder. With `[skills] sync`,
//! every skill found there, in a tool's own folder or in a `sources`
//! folder (a git checkout such as `~/.hub-skills/skills`) is linked into
//! every installed tool's folder, as a symlink to the one real folder, so
//! a `git pull` there or an edit reaches every tool at once.
//!
//! Each provider reads skills from its own folder, so a skill installed for
//! Claude Code is missing for opencode or pi until it is copied there. This
//! module lists what each provider has, copies a skill between them, packs
//! one for another device, and turns a `/skill args` tracker instruction
//! into words every provider follows (a slash command only works as the
//! very first thing typed, and only in some tools).

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Providers and the folder each one loads skills from.
pub fn provider_dirs() -> Vec<(&'static str, PathBuf)> {
    let home = dirs::home_dir().unwrap_or_default();
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    vec![
        ("shared", home.join(".agents/skills")),
        ("claude", env("CLAUDE_CONFIG_DIR").unwrap_or_else(|| home.join(".claude")).join("skills")),
        ("codex", env("CODEX_HOME").unwrap_or_else(|| home.join(".codex")).join("skills")),
        ("opencode", env("OPENCODE_CONFIG_DIR").unwrap_or_else(|| home.join(".config/opencode")).join("skills")),
        ("pi", env("PI_CODING_AGENT_DIR").unwrap_or_else(|| home.join(".pi/agent")).join("skills")),
        ("omp", home.join(".omp/agent/skills")),
        ("gemini", env("GEMINI_CLI_HOME").unwrap_or(home).join(".gemini/skills")),
    ]
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Skill {
    pub name: String,
    pub description: String,
    /// Providers on this device that have it.
    pub providers: Vec<String>,
    /// One folder that holds it (the first provider's), for `show` and copies.
    pub path: String,
}

/// Names are folder names: letters, digits, `-`, `_`, `.`, `:` (plugin skills).
pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 80 && !name.starts_with('.') && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// The `description:` of a SKILL.md front matter, on one line (plain,
/// quoted, or a YAML block scalar such as `>-` with indented lines).
pub fn description_of(skill_md: &str) -> String {
    let mut lines = skill_md.lines();
    if lines.next().map(str::trim) != Some("---") {
        return String::new();
    }
    let mut lines = lines.take_while(|l| l.trim() != "---");
    while let Some(l) = lines.next() {
        let Some(v) = l.strip_prefix("description:") else { continue };
        let v = v.trim();
        if v.is_empty() || v.starts_with('>') || v.starts_with('|') {
            let body: Vec<&str> = lines.by_ref().take_while(|l| l.is_empty() || l.starts_with(' ') || l.starts_with('\t')).map(str::trim).filter(|l| !l.is_empty()).collect();
            return body.join(" ");
        }
        return v.trim_matches('"').trim_matches('\'').to_string();
    }
    String::new()
}

/// Every skill on this device, with the providers that have it.
pub fn list() -> Vec<Skill> {
    list_in(&provider_dirs())
}

pub fn list_in(dirs: &[(&str, PathBuf)]) -> Vec<Skill> {
    let mut out: Vec<Skill> = Vec::new();
    for (provider, dir) in dirs {
        let Ok(rd) = std::fs::read_dir(dir) else { continue };
        let mut names: Vec<(String, PathBuf)> = rd
            .flatten()
            .map(|e| (e.file_name().to_string_lossy().to_string(), e.path()))
            .filter(|(n, p)| valid_name(n) && p.join("SKILL.md").is_file())
            .collect();
        names.sort();
        for (name, path) in names {
            match out.iter_mut().find(|s| s.name == name) {
                Some(s) => s.providers.push(provider.to_string()),
                None => out.push(Skill {
                    description: description_of(&std::fs::read_to_string(path.join("SKILL.md")).unwrap_or_default()),
                    name,
                    providers: vec![provider.to_string()],
                    path: path.display().to_string(),
                }),
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The folder of a skill on this device, from any provider.
pub fn find(name: &str) -> Option<PathBuf> {
    if !valid_name(name) {
        return None;
    }
    provider_dirs().into_iter().map(|(_, d)| d.join(name)).find(|p| p.join("SKILL.md").is_file())
}

/// Copy a skill folder into the providers in `to` that do not have it yet
/// (`all` = every provider whose tool is installed here, or whose skill
/// folder exists). Symlinks are followed, so the copy stands on its own.
/// Returns the providers it was added to.
pub fn copy(src: &Path, name: &str, to: &[String]) -> Result<Vec<String>> {
    copy_in(&provider_dirs(), src, name, to)
}

pub fn copy_in(dirs: &[(&str, PathBuf)], src: &Path, name: &str, to: &[String]) -> Result<Vec<String>> {
    if !valid_name(name) {
        return Err(anyhow!("not a skill name: {name}"));
    }
    if !src.join("SKILL.md").is_file() {
        return Err(anyhow!("{} has no SKILL.md", src.display()));
    }
    let all = to.iter().any(|t| t == "all");
    let mut added = Vec::new();
    for (provider, dir) in dirs {
        let wanted = if all { dir.parent().is_some_and(|p| p.is_dir()) } else { to.iter().any(|t| t == provider) };
        if !wanted {
            continue;
        }
        let dest = dir.join(name);
        if dest.join("SKILL.md").is_file() {
            continue;
        }
        if std::fs::symlink_metadata(&dest).is_ok() {
            return Err(anyhow!("{} exists but has no SKILL.md; move it away first", dest.display()));
        }
        std::fs::create_dir_all(dir)?;
        copy_dir(src, &dest).with_context(|| format!("copy {name} to {}", dest.display()))?;
        added.push(provider.to_string());
    }
    Ok(added)
}

fn copy_dir(src: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    for e in std::fs::read_dir(src)?.flatten() {
        let name = e.file_name();
        if name == ".git" || name == ".DS_Store" {
            continue;
        }
        let from = e.path();
        let to = dest.join(&name);
        // metadata() follows symlinks: a linked file or folder is copied as what it points at.
        let md = std::fs::metadata(&from)?;
        if md.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// `[skills]` in hubcodes.toml.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillsConfig {
    /// Link every skill into every installed tool's folder (and the shared one).
    #[serde(default)]
    pub sync: bool,
    /// Extra folders of skills, linked in too: a git checkout such as
    /// `~/.hub-skills/skills`, so a pull reaches every tool.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
}

/// Where `hubcodes skill add <git url>` clones; every checkout here is a source.
pub fn repos_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".local/share/hubcodes/skill-repos")
}

fn expand(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => dirs::home_dir().unwrap_or_default().join(rest),
        None => PathBuf::from(p),
    }
}

/// The skills under a folder: the folder itself, its children, or a
/// `skills/` folder in it (a repository of skills), skipping `.git`.
pub fn scan(dir: &Path) -> Vec<(String, PathBuf)> {
    if dir.join("SKILL.md").is_file() {
        let name = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        return if valid_name(&name) { vec![(name, dir.to_path_buf())] } else { vec![] };
    }
    let mut out = Vec::new();
    for base in [dir.to_path_buf(), dir.join("skills")] {
        let Ok(rd) = std::fs::read_dir(&base) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if valid_name(&name) && e.path().join("SKILL.md").is_file() && !out.iter().any(|(n, _)| *n == name) {
                out.push((name, e.path()));
            }
        }
    }
    out.sort();
    out
}

#[derive(Serialize, Deserialize, Default)]
struct Manifest {
    /// Links this module made: link path → the folder it points at.
    links: std::collections::BTreeMap<String, String>,
}

fn manifest_path() -> PathBuf {
    crate::db::state_dir().join("skill-links.json")
}

/// Link every skill into every installed tool and the shared folder; remove
/// links this made whose skill is gone. Returns what changed, one line each.
pub fn sync(cfg: &SkillsConfig) -> Vec<String> {
    sync_only(cfg, None)
}

/// [`sync`] for one skill (`Some(name)`), or for all.
pub fn sync_only(cfg: &SkillsConfig, only: Option<&str>) -> Vec<String> {
    let mut sources: Vec<PathBuf> = cfg.sources.iter().map(|s| expand(s)).collect();
    if let Ok(rd) = std::fs::read_dir(repos_dir()) {
        sources.extend(rd.flatten().map(|e| e.path()));
    }
    sync_in(&provider_dirs(), &sources, &manifest_path(), only)
}

pub fn sync_in(dirs: &[(&str, PathBuf)], sources: &[PathBuf], manifest: &Path, only: Option<&str>) -> Vec<String> {
    let mut man: Manifest = std::fs::read_to_string(manifest).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let mut changes = Vec::new();
    // Links whose skill is gone (a source pulled without it, a folder deleted).
    man.links.retain(|link, target| {
        let link = Path::new(link);
        let ours = std::fs::read_link(link).map(|t| t == Path::new(target)).unwrap_or(false);
        if ours && Path::new(target).join("SKILL.md").is_file() {
            return true;
        }
        if ours && std::fs::remove_file(link).is_ok() {
            changes.push(format!("removed {} (its skill is gone)", link.display()));
        }
        false
    });
    // Every skill once, by its real folder: sources first, then the shared folder, then each tool's.
    let mut skills: Vec<(String, PathBuf)> = Vec::new();
    let found = sources.iter().flat_map(|s| scan(s)).chain(dirs.iter().flat_map(|(_, d)| scan_children(d)));
    for (name, path) in found {
        if only.is_some_and(|o| o != name) || skills.iter().any(|(n, _)| *n == name) {
            continue;
        }
        if let Ok(real) = std::fs::canonicalize(&path) {
            skills.push((name, real));
        }
    }
    for (provider, dir) in dirs {
        let installed = *provider == "shared" || dir.is_dir() || (dir.parent().is_some_and(|p| p.is_dir()) && on_path(provider));
        if !installed {
            continue;
        }
        for (name, real) in &skills {
            let link = dir.join(name);
            if std::fs::symlink_metadata(&link).is_ok() {
                continue;
            }
            if std::fs::create_dir_all(dir).is_err() {
                break;
            }
            if link_dir(real, &link).is_ok() {
                man.links.insert(link.display().to_string(), real.display().to_string());
                changes.push(format!("{name} → {provider}"));
            }
        }
    }
    if let Some(d) = manifest.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(manifest, serde_json::to_string_pretty(&man).unwrap_or_default());
    changes
}

/// Is the tool's command on PATH? A leftover config folder alone does not
/// make a tool installed.
fn on_path(cmd: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(cmd).is_file()))
}

fn scan_children(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    let mut v: Vec<(String, PathBuf)> = rd.flatten().map(|e| (e.file_name().to_string_lossy().to_string(), e.path())).filter(|(n, p)| valid_name(n) && p.join("SKILL.md").is_file()).collect();
    v.sort();
    v
}

#[cfg(unix)]
fn link_dir(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}
#[cfg(not(unix))]
fn link_dir(target: &Path, link: &Path) -> std::io::Result<()> {
    copy_dir(target, link).map_err(|e| std::io::Error::other(e.to_string()))
}

/// The skill that teaches any coding agent to install, set up and run
/// hubcodes and its skills commands, so the user can just ask for it.
pub const BUILTIN: &str = include_str!("assets/hubcodes-skill.md");
const BUILTIN_MARK: &str = "hubcodes:builtin";

/// Write the built-in `hubcodes` skill into the shared folder. A copy this
/// wrote is refreshed when it differs; a folder the user made is left alone.
/// Returns a line when something changed.
pub fn ensure_builtin() -> Option<String> {
    let shared = provider_dirs().into_iter().find(|(p, _)| *p == "shared").map(|(_, d)| d)?;
    ensure_builtin_in(&shared)
}

pub fn ensure_builtin_in(shared: &Path) -> Option<String> {
    let dir = shared.join("hubcodes");
    let file = dir.join("SKILL.md");
    let now = std::fs::read_to_string(&file).ok();
    match &now {
        Some(t) if t == BUILTIN => return None,
        Some(t) if !t.contains(BUILTIN_MARK) => return None,
        _ => {}
    }
    if dir.is_symlink() {
        return None;
    }
    std::fs::create_dir_all(&dir).ok()?;
    std::fs::write(&file, BUILTIN).ok()?;
    Some(format!("{} skill hubcodes in {}", if now.is_some() { "updated" } else { "added" }, shared.display()))
}

/// Turn sync on and add or drop skill folders. Returns what changed.
pub fn configure(cfg: &mut SkillsConfig, sync: bool, add: &[String], drop: &[String]) -> Vec<String> {
    let mut changes = Vec::new();
    if cfg.sync != sync {
        cfg.sync = sync;
        changes.push(format!("skills sync {}", if sync { "on" } else { "off" }));
    }
    let same = |a: &str, b: &str| expand(a) == expand(b);
    for d in drop {
        let before = cfg.sources.len();
        cfg.sources.retain(|s| !same(s, d));
        if cfg.sources.len() != before {
            changes.push(format!("stopped linking {d}"));
        }
    }
    for a in add.iter().map(|a| a.trim()).filter(|a| !a.is_empty()) {
        if !cfg.sources.iter().any(|s| same(s, a)) {
            cfg.sources.push(a.to_string());
            changes.push(format!("linking skills from {a}"));
        }
    }
    changes
}

/// Add a skill to this device's shared folder from:
/// - a folder (with SKILL.md, or holding several skills),
/// - a single `SKILL.md` (or any `.md` with a `name:` front matter),
/// - a `.zip` / `.skill` file downloaded from a browser,
/// - a web link to one of those (`https://…/skill.zip`, a raw SKILL.md),
/// - a GitHub link to one skill (`https://github.com/o/r/tree/main/skills/x`),
/// - a git repository (cloned under [`repos_dir`], kept as a source, so
///   `hubcodes skill pull` brings its updates).
///
/// Returns the names added. Then [`sync`] links them into every tool.
pub fn add(source: &str) -> Result<Vec<String>> {
    let shared = provider_dirs().into_iter().find(|(p, _)| *p == "shared").map(|(_, d)| d).unwrap_or_default();
    add_into(&shared, &repos_dir(), source.trim())
}

/// What a source string is.
#[derive(Debug, PartialEq)]
pub enum Source {
    /// A whole repository of skills: clone it and keep it as a source.
    Repo(String),
    /// One folder of a GitHub repository: (clone URL, branch, path in it).
    GithubPath(String, String, String),
    /// A file to download (zip, .skill or a raw SKILL.md).
    Download(String),
    Local(PathBuf),
}

pub fn classify(src: &str) -> Source {
    if src.starts_with("git@") || (src.starts_with("https://") && src.trim_end_matches('/').ends_with(".git")) {
        return Source::Repo(src.to_string());
    }
    if let Some(rest) = src.strip_prefix("https://github.com/") {
        let parts: Vec<&str> = rest.trim_end_matches('/').split('/').collect();
        match parts.as_slice() {
            [o, r] => return Source::Repo(format!("https://github.com/{o}/{r}.git")),
            [o, r, "tree" | "blob", branch, path @ ..] if !path.is_empty() => {
                let mut path = path.join("/");
                // A link to the SKILL.md itself: its folder.
                if path.ends_with(".md") {
                    path = path.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
                }
                return Source::GithubPath(format!("https://github.com/{o}/{r}.git"), branch.to_string(), path);
            }
            _ => {}
        }
    }
    if src.starts_with("https://") || src.starts_with("http://") {
        return Source::Download(src.to_string());
    }
    Source::Local(expand(src))
}

fn git_clone(url: &str, branch: Option<&str>, dest: &Path) -> Result<()> {
    let mut c = Command::new("git");
    c.args(["clone", "--depth", "1"]);
    if let Some(b) = branch {
        c.args(["--branch", b]);
    }
    let out = c.args(["--", url]).arg(dest).output()?;
    if !out.status.success() {
        return Err(anyhow!("git clone {url}: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(())
}

/// The `name:` of a SKILL.md front matter.
pub fn name_of(skill_md: &str) -> Option<String> {
    let mut lines = skill_md.lines();
    if lines.next().map(str::trim) != Some("---") {
        return None;
    }
    lines.take_while(|l| l.trim() != "---").find_map(|l| l.strip_prefix("name:")).map(|v| v.trim().trim_matches('"').trim_matches('\'').to_string()).filter(|n| valid_name(n))
}

pub fn add_into(shared: &Path, repos: &Path, src: &str) -> Result<Vec<String>> {
    let tmp = std::env::temp_dir().join(format!("hubcodes-skill-add-{}-{}", std::process::id(), crate::db::now()));
    let res = (|| {
        let dir: PathBuf = match classify(src) {
            Source::Repo(url) => {
                let repo = url.trim_end_matches('/').trim_end_matches(".git").rsplit(['/', ':']).next().unwrap_or("repo").to_string();
                if !valid_name(&repo) {
                    return Err(anyhow!("cannot name a folder after {url}"));
                }
                let dest = repos.join(&repo);
                if !dest.exists() {
                    std::fs::create_dir_all(repos)?;
                    git_clone(&url, None, &dest)?;
                }
                let names: Vec<String> = scan(&dest).into_iter().map(|(n, _)| n).collect();
                if names.is_empty() {
                    let _ = std::fs::remove_dir_all(&dest);
                    return Err(anyhow!("{url} has no SKILL.md (at its root, in a folder, or under skills/)"));
                }
                return Ok(names);
            }
            Source::GithubPath(url, branch, path) => {
                git_clone(&url, Some(&branch), &tmp.join("repo"))?;
                tmp.join("repo").join(path)
            }
            Source::Download(url) => {
                std::fs::create_dir_all(&tmp)?;
                let file = tmp.join("download");
                let out = Command::new("curl").args(["-fsSL", "--max-time", "60", "-o"]).arg(&file).arg(&url).output()?;
                if !out.status.success() {
                    return Err(anyhow!("download {url}: {}", String::from_utf8_lossy(&out.stderr).trim()));
                }
                unpack_file(&file, &tmp.join("x"), url.rsplit('/').next().unwrap_or(""))?
            }
            Source::Local(p) if p.is_file() => {
                std::fs::create_dir_all(&tmp)?;
                unpack_file(&p, &tmp.join("x"), &p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())?
            }
            Source::Local(p) => p,
        };
        let found = scan(&dir);
        if found.is_empty() {
            return Err(anyhow!("no SKILL.md in {src}"));
        }
        let mut names = Vec::new();
        for (name, p) in found {
            let dest = shared.join(&name);
            if std::fs::symlink_metadata(&dest).is_ok() {
                return Err(anyhow!("{name} is already in {}; remove it there first to replace it", shared.display()));
            }
            std::fs::create_dir_all(shared)?;
            copy_dir(&p, &dest)?;
            names.push(name);
        }
        Ok(names)
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    res
}

/// A zip (`.zip`, `.skill`) or a markdown file, into folder `out`; returns
/// the folder to scan. A lone SKILL.md becomes `<name>/SKILL.md`, the name
/// from its front matter or else from the file name.
fn unpack_file(file: &Path, out: &Path, file_name: &str) -> Result<PathBuf> {
    let bytes = std::fs::read(file)?;
    std::fs::create_dir_all(out)?;
    if bytes.starts_with(b"PK") {
        // macOS has ditto; elsewhere unzip, or Python's zipfile when unzip is missing.
        let o = if cfg!(target_os = "macos") {
            Command::new("/usr/bin/ditto").args(["-x", "-k"]).arg(file).arg(out).output()
        } else {
            Command::new("unzip").args(["-q", "-o"]).arg(file).arg("-d").arg(out).output().or_else(|_| Command::new("python3").args(["-m", "zipfile", "-e"]).arg(file).arg(out).output())
        }
        .with_context(|| format!("unzip {file_name}: no ditto, unzip or python3"))?;
        if !o.status.success() {
            return Err(anyhow!("unzip {file_name}: {}", String::from_utf8_lossy(&o.stderr).trim()));
        }
        return Ok(out.to_path_buf());
    }
    let text = String::from_utf8(bytes).map_err(|_| anyhow!("{file_name} is neither a zip nor a SKILL.md"))?;
    let stem = file_name.trim_end_matches(".md").trim_end_matches(".MD");
    let name = name_of(&text).or_else(|| (valid_name(stem) && !stem.eq_ignore_ascii_case("skill")).then(|| stem.to_string())).ok_or_else(|| anyhow!("{file_name}: give the skill a `name:` in its front matter"))?;
    if !text.trim_start().starts_with("---") {
        return Err(anyhow!("{file_name} has no front matter (--- name / description ---), so no tool would load it"));
    }
    std::fs::create_dir_all(out.join(&name))?;
    std::fs::write(out.join(&name).join("SKILL.md"), text)?;
    Ok(out.to_path_buf())
}

/// `git pull --ff-only` in every source that is a git checkout. Returns one line each.
pub fn pull(cfg: &SkillsConfig) -> Vec<String> {
    let mut roots: Vec<PathBuf> = cfg.sources.iter().map(|s| expand(s)).collect();
    if let Ok(rd) = std::fs::read_dir(repos_dir()) {
        roots.extend(rd.flatten().map(|e| e.path()));
    }
    let mut out = Vec::new();
    for r in roots {
        let top = Command::new("git").arg("-C").arg(&r).args(["rev-parse", "--show-toplevel"]).output();
        let Some(top) = top.ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()) else { continue };
        let res = Command::new("git").args(["-C", &top, "pull", "--ff-only", "-q"]).output();
        out.push(match res {
            Ok(o) if o.status.success() => format!("{top}: up to date"),
            Ok(o) => format!("{top}: {}", String::from_utf8_lossy(&o.stderr).trim().lines().last().unwrap_or("pull failed")),
            Err(e) => format!("{top}: {e}"),
        });
    }
    out
}

/// A skill as base64 tar text (`hubcodes skill pack`), for another device.
pub fn pack(name: &str) -> Result<String> {
    pack_dir(&find(name).ok_or_else(|| anyhow!("no skill {name} on this device"))?, name)
}

fn pack_dir(dir: &Path, name: &str) -> Result<String> {
    let parent = dir.parent().ok_or_else(|| anyhow!("no parent"))?;
    let tar = Command::new("tar").args(["-chf", "-", "--exclude", ".git", "-C"]).arg(parent).arg(name).stderr(Stdio::piped()).output()?;
    if !tar.status.success() {
        return Err(anyhow!("tar {name}: {}", String::from_utf8_lossy(&tar.stderr).trim()));
    }
    Ok(crate::connections::base64(&tar.stdout))
}

/// Unpack what [`pack`] gave (on another device) and add it to `to`.
pub fn unpack(name: &str, packed: &str, to: &[String]) -> Result<Vec<String>> {
    unpack_in(&provider_dirs(), name, packed, to)
}

pub fn unpack_in(dirs: &[(&str, PathBuf)], name: &str, packed: &str, to: &[String]) -> Result<Vec<String>> {
    if !valid_name(name) {
        return Err(anyhow!("not a skill name: {name}"));
    }
    let tmp = std::env::temp_dir().join(format!("hubcodes-skill-{}-{}", std::process::id(), crate::db::now()));
    std::fs::create_dir_all(&tmp)?;
    let res = (|| {
        let bytes = crate::connections::base64_decode(packed.trim()).ok_or_else(|| anyhow!("the packed skill is not base64"))?;
        let mut tar = Command::new("tar").args(["-xf", "-", "-C"]).arg(&tmp).stdin(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
        std::io::Write::write_all(tar.stdin.as_mut().ok_or_else(|| anyhow!("tar stdin"))?, &bytes)?;
        let out = tar.wait_with_output()?;
        if !out.status.success() {
            return Err(anyhow!("untar {name}: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        copy_in(dirs, &tmp.join(name), name, to)
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    res
}

/// A tracker instruction that starts with `/name args` and names a skill:
/// `(name, args)`. Saved prompts and unknown commands are left alone.
pub fn command_of(line: &str, known: impl Fn(&str) -> bool) -> Option<(String, String)> {
    let rest = line.trim_start().strip_prefix('/')?;
    let (name, args) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    (valid_name(name) && known(name)).then(|| (name.to_string(), args.trim().to_string()))
}

/// The words that make any provider run the skill: a slash command is only
/// read as one when it is the first thing typed, which a hubcodes
/// assignment never is.
pub fn invocation(name: &str, args: &str) -> String {
    let what = if args.is_empty() { String::new() } else { format!(" on {args}") };
    format!("Use the `{name}` skill{what}: load it with your skill tool (in Claude Code the Skill tool) and follow it. If you do not have that skill, run `hubcodes skill show {name}` and follow what it prints.")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hubcodes-skills-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn lists_merges_and_copies_across_providers() {
        let root = tmp("copy");
        let claude = root.join("claude/skills");
        let pi = root.join("pi/skills");
        std::fs::create_dir_all(claude.join("pr-review/refs")).unwrap();
        std::fs::write(claude.join("pr-review/SKILL.md"), "---\nname: pr-review\ndescription: \"Reviews PRs\"\n---\nbody").unwrap();
        std::fs::write(claude.join("pr-review/refs/a.md"), "ref").unwrap();
        std::fs::create_dir_all(claude.join("no-skill-md")).unwrap();
        std::fs::create_dir_all(root.join("pi")).unwrap();
        let dirs = vec![("claude", claude.clone()), ("pi", pi.clone()), ("omp", root.join("omp/skills"))];
        let l = list_in(&dirs);
        assert_eq!(l.len(), 1, "a folder without SKILL.md is not a skill");
        assert_eq!((l[0].description.as_str(), l[0].providers.clone()), ("Reviews PRs", vec!["claude".to_string()]));
        let added = copy_in(&dirs, &claude.join("pr-review"), "pr-review", &["all".into()]).unwrap();
        assert_eq!(added, vec!["pi"], "all = providers whose folder is there; omp is not installed");
        assert_eq!(std::fs::read_to_string(pi.join("pr-review/refs/a.md")).unwrap(), "ref");
        assert!(copy_in(&dirs, &claude.join("pr-review"), "pr-review", &["all".into()]).unwrap().is_empty(), "never twice");
        assert_eq!(list_in(&dirs)[0].providers, vec!["claude", "pi"]);
        assert!(copy_in(&dirs, &claude.join("pr-review"), "../x", &["pi".into()]).is_err());
        // Another device: packed there, unpacked here.
        let packed = pack_dir(&claude.join("pr-review"), "pr-review").unwrap();
        std::fs::create_dir_all(root.join("omp")).unwrap();
        assert_eq!(unpack_in(&dirs, "pr-review", &packed, &["omp".into()]).unwrap(), vec!["omp"]);
        assert_eq!(std::fs::read_to_string(root.join("omp/skills/pr-review/refs/a.md")).unwrap(), "ref");
    }

    #[test]
    fn sources_are_told_apart() {
        assert_eq!(classify("https://github.com/anthropics/skills"), Source::Repo("https://github.com/anthropics/skills.git".into()));
        assert_eq!(classify("git@github.com:o/r.git"), Source::Repo("git@github.com:o/r.git".into()));
        assert_eq!(classify("https://github.com/anthropics/skills/tree/main/skills/pdf"), Source::GithubPath("https://github.com/anthropics/skills.git".into(), "main".into(), "skills/pdf".into()));
        assert_eq!(classify("https://github.com/o/r/blob/main/x/SKILL.md"), Source::GithubPath("https://github.com/o/r.git".into(), "main".into(), "x".into()));
        assert_eq!(classify("https://example.com/a.zip"), Source::Download("https://example.com/a.zip".into()));
        assert!(matches!(classify("~/Downloads/a.skill"), Source::Local(_)));
    }

    #[test]
    fn a_lone_skill_md_a_zip_and_a_folder_can_be_added() {
        let root = tmp("add");
        let shared = root.join("agents/skills");
        let md = root.join("SKILL.md");
        std::fs::write(&md, "---\nname: release-notes\ndescription: Writes notes\n---\nbody").unwrap();
        assert_eq!(add_into(&shared, &root.join("repos"), md.to_str().unwrap()).unwrap(), vec!["release-notes"]);
        assert!(shared.join("release-notes/SKILL.md").is_file());
        assert!(add_into(&shared, &root.join("repos"), md.to_str().unwrap()).is_err(), "never over an existing one");
        let plain = root.join("notes.md");
        std::fs::write(&plain, "just text").unwrap();
        assert!(add_into(&shared, &root.join("repos"), plain.to_str().unwrap()).is_err(), "no front matter: not a skill");
        std::fs::create_dir_all(root.join("src/zipped")).unwrap();
        std::fs::write(root.join("src/zipped/SKILL.md"), "---\nname: zipped\n---\n").unwrap();
        let zip = root.join("zipped.skill");
        let made = if cfg!(target_os = "macos") {
            Command::new("/usr/bin/ditto").args(["-c", "-k", "--keepParent"]).arg(root.join("src/zipped")).arg(&zip).status()
        } else {
            Command::new("python3").args(["-m", "zipfile", "-c"]).arg(&zip).arg(root.join("src/zipped")).status()
        };
        assert!(made.unwrap().success());
        assert_eq!(add_into(&shared, &root.join("repos"), zip.to_str().unwrap()).unwrap(), vec!["zipped"]);
        assert!(shared.join("zipped/SKILL.md").is_file());
    }

    #[test]
    fn descriptions_read_plain_quoted_and_folded() {
        assert_eq!(description_of("---\nname: a\ndescription: >-\n  Line one\n  line two.\nmetadata: x\n---\n"), "Line one line two.");
        assert_eq!(description_of("---\ndescription: |\n  A\n---"), "A");
        assert_eq!(description_of("---\ndescription: 'Quoted'\n---"), "Quoted");
        assert_eq!(description_of("no front matter"), "");
    }

    #[test]
    fn sync_links_every_skill_everywhere_and_cleans_up() {
        let root = tmp("sync");
        let repo = root.join("hub-skills/skills");
        std::fs::create_dir_all(repo.join("pr-review")).unwrap();
        std::fs::write(repo.join("pr-review/SKILL.md"), "v1").unwrap();
        let claude = root.join("claude/skills");
        std::fs::create_dir_all(claude.join("mine")).unwrap();
        std::fs::write(claude.join("mine/SKILL.md"), "x").unwrap();
        std::fs::create_dir_all(root.join("pi/skills")).unwrap();
        let dirs = vec![("shared", root.join("agents/skills")), ("claude", claude.clone()), ("pi", root.join("pi/skills")), ("omp", root.join("omp/skills"))];
        let man = root.join("state/links.json");
        let ch = sync_in(&dirs, &[root.join("hub-skills")], &man, None);
        assert_eq!(ch.len(), 5, "{ch:?}");
        for d in ["agents/skills", "claude/skills", "pi/skills"] {
            assert!(root.join(d).join("pr-review/SKILL.md").is_file(), "{d}");
        }
        assert!(std::fs::symlink_metadata(claude.join("mine")).unwrap().is_dir(), "a real folder stays as it is");
        assert!(!root.join("omp").exists(), "a tool that is not installed is left alone");
        assert!(!root.join("pi/skills").exists() || root.join("pi/skills/pr-review").exists());
        std::fs::write(repo.join("pr-review/SKILL.md"), "v2").unwrap();
        assert_eq!(std::fs::read_to_string(root.join("pi/skills/pr-review/SKILL.md")).unwrap(), "v2", "a pull reaches every tool");
        assert!(sync_in(&dirs, &[root.join("hub-skills")], &man, None).is_empty(), "nothing twice");
        std::fs::remove_dir_all(repo.join("pr-review")).unwrap();
        let gone = sync_in(&dirs, &[root.join("hub-skills")], &man, None);
        assert_eq!(gone.len(), 3, "{gone:?}");
        assert!(std::fs::symlink_metadata(root.join("pi/skills/pr-review")).is_err());
        assert!(claude.join("mine/SKILL.md").is_file());
    }

    #[test]
    fn a_slash_instruction_becomes_words_every_provider_follows() {
        let known = |n: &str| n == "pr-review";
        assert_eq!(command_of("/pr-review https://x/1", known), Some(("pr-review".into(), "https://x/1".into())));
        assert_eq!(command_of("  /pr-review", known), Some(("pr-review".into(), String::new())));
        assert_eq!(command_of("/review {url}", known), None, "not a skill here: left alone");
        assert_eq!(command_of("Review https://x", known), None);
        let s = invocation("pr-review", "https://x/1");
        assert!(s.starts_with("Use the `pr-review` skill on https://x/1") && s.contains("hubcodes skill show pr-review"), "{s}");
    }

    #[test]
    fn builtin_skill_is_written_refreshed_and_never_overwrites_the_users() {
        let tmp = std::env::temp_dir().join(format!("hc-builtin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        assert!(ensure_builtin_in(&tmp).unwrap().starts_with("added"));
        assert!(ensure_builtin_in(&tmp).is_none());
        let file = tmp.join("hubcodes/SKILL.md");
        std::fs::write(&file, format!("old {BUILTIN_MARK}")).unwrap();
        assert!(ensure_builtin_in(&tmp).unwrap().starts_with("updated"));
        std::fs::write(&file, "---\nname: hubcodes\n---\nmine").unwrap();
        assert!(ensure_builtin_in(&tmp).is_none());
        assert!(std::fs::read_to_string(&file).unwrap().ends_with("mine"));
        assert_eq!(name_of(BUILTIN).as_deref(), Some("hubcodes"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn configure_turns_sync_on_and_dedupes_sources() {
        let mut c = SkillsConfig { sync: false, sources: vec!["~/a".into()] };
        let ch = configure(&mut c, true, &["~/a".into(), "~/b".into()], &[]);
        assert_eq!(ch, vec!["skills sync on".to_string(), "linking skills from ~/b".into()]);
        assert!(configure(&mut c, true, &["~/b".into()], &[]).is_empty());
        assert_eq!(configure(&mut c, true, &[], &["~/a".into()]), vec!["stopped linking ~/a".to_string()]);
        assert_eq!(c.sources, vec!["~/b".to_string()]);
    }

}
