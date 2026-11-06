//! `shape`: the ShapesDB command line. Git does the version control; `shape`
//! adds ShapesDB accounts, model validation, model-aware diffs and publishing.

mod api;
mod config;
mod git;
mod model;

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};
use serde_json::json;

#[derive(Parser)]
#[command(name = "shape", version, about = "Version control and collaboration for 3D, on top of Git.")]
struct Cli {
    /// ShapesDB host (defaults to your configured host or https://shapesdb.com).
    #[arg(long, global = true, env = "SHAPESDB_HOST")]
    host: Option<String>,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Sign in with a personal access token and configure Git to use it.
    Login {
        /// Token (otherwise read from SHAPESDB_TOKEN or prompted).
        #[arg(long, env = "SHAPESDB_TOKEN", hide_env_values = true)]
        token: Option<String>,
        /// Do not configure the Git credential helper.
        #[arg(long)]
        no_git_setup: bool,
    },
    /// Remove stored credentials for a host.
    Logout,
    /// Show the signed-in user.
    Whoami,
    /// Create a Git repository set up for 3D assets (LFS tracking for model formats).
    Init { dir: Option<PathBuf> },
    /// Clone owner/repository and fetch LFS content.
    Clone { repository: String, dir: Option<PathBuf> },
    /// `git status`
    Status,
    /// `git add`, warning about model files that are not stored with LFS.
    Add {
        #[arg(required = true)]
        paths: Vec<String>,
    },
    /// `git commit`
    Commit {
        #[arg(short, long)]
        message: String,
    },
    /// `git push`
    Push { args: Vec<String> },
    /// `git pull`
    Pull { args: Vec<String> },
    /// Show changes, with geometry statistics for model files.
    Diff {
        /// Compare against this revision (default HEAD).
        rev: Option<String>,
    },
    /// Inspect one model file.
    Inspect {
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Validate model files (defaults to every model file in the repository).
    Validate { paths: Vec<PathBuf> },
    /// Open the repository (or a file) on ShapesDB in your browser.
    Preview { path: Option<String> },
    /// Create a ShapesDB repository for the current directory and push it.
    Publish {
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        owner: Option<String>,
        #[arg(long)]
        private: bool,
        #[arg(long, default_value = "")]
        description: String,
    },
    /// Create a release (and tag) on ShapesDB.
    Release {
        tag: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long, default_value = "")]
        notes: String,
        /// Branch or commit to tag (default: the default branch).
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        prerelease: bool,
    },
    /// Internal: Git credential helper protocol.
    #[command(hide = true)]
    GitCredential { action: String },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(1)
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    let mut cfg = config::Config::load()?;
    let host = cfg.host(cli.host.as_deref());
    match cli.command {
        Cmd::Login { token, no_git_setup } => login(&mut cfg, &host, token, !no_git_setup),
        Cmd::Logout => {
            cfg.hosts.remove(&host);
            cfg.save()?;
            println!("Signed out of {host}.");
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Whoami => {
            let me = client(&cfg, &host)?.me()?;
            println!("{} on {host}", me.username);
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Init { dir } => init(dir),
        Cmd::Clone { repository, dir } => clone(&cfg, &host, &repository, dir),
        Cmd::Status => passthrough(&["status"]),
        Cmd::Add { paths } => add(&paths),
        Cmd::Commit { message } => passthrough(&["commit", "-m", &message]),
        Cmd::Push { args } => passthrough(&[&["push"][..], &args.iter().map(String::as_str).collect::<Vec<_>>()].concat()),
        Cmd::Pull { args } => passthrough(&[&["pull"][..], &args.iter().map(String::as_str).collect::<Vec<_>>()].concat()),
        Cmd::Diff { rev } => diff(rev.as_deref().unwrap_or("HEAD")),
        Cmd::Inspect { path, json } => inspect(&path, json),
        Cmd::Validate { paths } => validate(paths),
        Cmd::Preview { path } => preview(path),
        Cmd::Publish { name, owner, private, description } => publish(&cfg, &host, name, owner, private, &description),
        Cmd::Release { tag, title, notes, target, prerelease } => release(&cfg, &host, &tag, title, &notes, target, prerelease),
        Cmd::GitCredential { action } => git_credential(&cfg, &action),
    }
}

fn client(cfg: &config::Config, host: &str) -> Result<api::Client> {
    let creds = cfg.credentials(host).ok_or_else(|| anyhow!("not signed in to {host}; run `shape login`"))?;
    Ok(api::Client::new(host, Some(creds.token.clone())))
}

fn passthrough(args: &[&str]) -> Result<ExitCode> {
    let status = git::run(args)?;
    Ok(ExitCode::from(status.code().unwrap_or(1) as u8))
}

fn login(cfg: &mut config::Config, host: &str, token: Option<String>, git_setup: bool) -> Result<ExitCode> {
    let token = match token {
        Some(t) => t,
        None => {
            println!("Create a token at {host}/settings/tokens (scopes: repo:write, user).");
            rpassword::prompt_password("Token: ")?
        }
    };
    let token = token.trim().to_string();
    let me = api::Client::new(host, Some(token.clone())).me().context("token was rejected")?;
    cfg.hosts.insert(host.to_string(), config::HostConfig { username: me.username.clone(), token });
    cfg.default_host.get_or_insert_with(|| host.to_string());
    cfg.save()?;
    if git_setup {
        let key = format!("credential.{host}.helper");
        git::run_ok(&["config", "--global", "--replace-all", &key, "!shape git-credential"])?;
        println!("Configured Git to use shape for {host}.");
    }
    println!("Signed in to {host} as {}.", me.username);
    Ok(ExitCode::SUCCESS)
}

/// Implements Git's credential helper protocol for configured hosts.
fn git_credential(cfg: &config::Config, action: &str) -> Result<ExitCode> {
    if action != "get" {
        return Ok(ExitCode::SUCCESS); // store/erase: credentials are managed by `shape login`
    }
    let mut protocol = String::new();
    let mut hostname = String::new();
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once('=') {
            match k {
                "protocol" => protocol = v.to_string(),
                "host" => hostname = v.to_string(),
                _ => {}
            }
        }
    }
    if let Some(c) = cfg.credentials(&format!("{protocol}://{hostname}")) {
        println!("username={}\npassword={}", c.username, c.token);
    }
    Ok(ExitCode::SUCCESS)
}

const LFS_PATTERNS: &[&str] = &["*.glb", "*.gltf.bin", "*.bin", "*.fbx", "*.blend", "*.obj", "*.stl", "*.ply", "*.3mf", "*.step", "*.stp", "*.usd", "*.usdz", "*.usdc", "*.png", "*.jpg", "*.jpeg", "*.exr", "*.hdr", "*.tif", "*.ktx2"];

fn init(dir: Option<PathBuf>) -> Result<ExitCode> {
    let dir = dir.unwrap_or_else(|| PathBuf::from("."));
    std::fs::create_dir_all(&dir)?;
    std::env::set_current_dir(&dir)?;
    if !git::in_repo() {
        git::run_ok(&["init", "-b", "main"])?;
    }
    if git::lfs_available() {
        git::run_ok(&["lfs", "install", "--local"])?;
        let mut args = vec!["lfs", "track"];
        args.extend(LFS_PATTERNS);
        git::run_ok(&args)?;
    } else {
        eprintln!("warning: git-lfs is not installed; large model files will be stored in Git directly.");
    }
    println!("Initialized a ShapesDB-ready repository in {}.", std::env::current_dir()?.display());
    Ok(ExitCode::SUCCESS)
}

fn clone(cfg: &config::Config, host: &str, repository: &str, dir: Option<PathBuf>) -> Result<ExitCode> {
    let repo: api::Repo = match cfg.credentials(host) {
        Some(c) => api::Client::new(host, Some(c.token.clone())),
        None => api::Client::new(host, None),
    }
    .get(&format!("/repos/{repository}"))?;
    let mut args = vec!["clone".to_string(), repo.clone_http.clone()];
    if let Some(d) = &dir {
        args.push(d.display().to_string());
    }
    let status = git::run(&args.iter().map(String::as_str).collect::<Vec<_>>())?;
    Ok(ExitCode::from(status.code().unwrap_or(1) as u8))
}

fn add(paths: &[String]) -> Result<ExitCode> {
    let mut args = vec!["add", "--"];
    args.extend(paths.iter().map(String::as_str));
    git::run_ok(&args)?;
    // Warn about staged model files that are plain Git blobs instead of LFS pointers.
    let staged = git::output(&["diff", "--cached", "--name-only"])?;
    for f in staged.lines().filter(|f| model::format_of(Path::new(f)).is_some()) {
        let attr = git::output(&["check-attr", "filter", "--", f]).unwrap_or_default();
        if !attr.ends_with("lfs") {
            eprintln!("warning: {f} is not tracked by Git LFS. Run `git lfs track \"*.{}\"`.", Path::new(f).extension().and_then(|e| e.to_str()).unwrap_or("*"));
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn fmt_info(i: &model::ModelInfo) -> String {
    let dims = i
        .bounds
        .map(|[mn, mx]| format!("{:.3} x {:.3} x {:.3}", mx[0] - mn[0], mx[1] - mn[1], mx[2] - mn[2]))
        .unwrap_or_else(|| "unknown".into());
    format!("{} tris, {} verts, {} meshes, {} materials, size {dims}", i.triangles, i.vertices, i.meshes, i.materials)
}

fn inspect(path: &Path, json: bool) -> Result<ExitCode> {
    let info = model::inspect(path)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&info)?);
    } else {
        println!("{}  ({}, {} bytes)", path.display(), info.format.to_uppercase(), info.size);
        println!("  {}", fmt_info(&info));
        if info.textures > 0 || info.animations > 0 {
            println!("  {} textures, {} animations", info.textures, info.animations);
        }
        for w in &info.warnings {
            println!("  warning: {w}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn validate(paths: Vec<PathBuf>) -> Result<ExitCode> {
    let files: Vec<PathBuf> = if paths.is_empty() {
        let root = git::repo_root().context("run inside a repository or pass file paths")?;
        let listed = git::output(&["ls-files", "--cached", "--others", "--exclude-standard", "-z"])?;
        listed.split('\0').filter(|s| !s.is_empty()).map(|s| root.join(s)).filter(|p| model::format_of(p).is_some()).collect()
    } else {
        paths
    };
    if files.is_empty() {
        println!("No model files found.");
        return Ok(ExitCode::SUCCESS);
    }
    let mut failed = 0;
    for f in &files {
        match model::inspect(f) {
            Ok(i) => {
                let mark = if i.warnings.is_empty() { "ok  " } else { "warn" };
                println!("{mark}  {}  {}", f.display(), fmt_info(&i));
                for w in &i.warnings {
                    println!("      {w}");
                }
            }
            Err(e) => {
                failed += 1;
                println!("FAIL  {}  {e:#}", f.display());
            }
        }
    }
    println!("{} files checked, {failed} invalid.", files.len());
    Ok(if failed > 0 { ExitCode::from(1) } else { ExitCode::SUCCESS })
}

fn diff(rev: &str) -> Result<ExitCode> {
    git::run_ok(&["diff", "--stat", rev])?;
    let names = git::output(&["diff", "--name-status", rev])?;
    let root = git::repo_root()?;
    for line in names.lines() {
        let mut parts = line.split('\t');
        let (Some(status), Some(path)) = (parts.next(), parts.next()) else { continue };
        if model::format_of(Path::new(path)).is_none() {
            continue;
        }
        println!("\n{path}");
        let before = if status.starts_with('A') { None } else { git::show_bytes(rev, path).ok() };
        let after = if status.starts_with('D') { None } else { Some(root.join(path)) };
        let tmp = tempfile_for(path, before.as_deref())?;
        let b = tmp.as_ref().and_then(|t| model::inspect(t.path()).ok());
        let a = after.as_ref().and_then(|p| model::inspect(p).ok());
        match (b, a) {
            (Some(b), Some(a)) => {
                for (k, x, y) in [("triangles", b.triangles, a.triangles), ("vertices", b.vertices, a.vertices), ("meshes", b.meshes, a.meshes), ("materials", b.materials, a.materials)] {
                    if x != y {
                        println!("  {k:<10} {x} -> {y} ({:+})", y as i64 - x as i64);
                    }
                }
                if b.bounds != a.bounds {
                    println!("  bounds     changed");
                }
            }
            (None, Some(a)) => println!("  added: {}", fmt_info(&a)),
            (Some(b), None) => println!("  removed: {}", fmt_info(&b)),
            (None, None) => println!("  (could not inspect)"),
        }
    }
    Ok(ExitCode::SUCCESS)
}

struct TempModel {
    _dir: std::path::PathBuf,
    file: std::path::PathBuf,
}

impl TempModel {
    fn path(&self) -> &Path {
        &self.file
    }
}

impl Drop for TempModel {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self._dir);
    }
}

fn tempfile_for(path: &str, data: Option<&[u8]>) -> Result<Option<TempModel>> {
    let Some(data) = data else { return Ok(None) };
    let dir = std::env::temp_dir().join(format!("shape-diff-{}-{}", std::process::id(), path.len()));
    std::fs::create_dir_all(&dir)?;
    let file = dir.join(Path::new(path).file_name().unwrap_or_default());
    std::fs::File::create(&file)?.write_all(data)?;
    Ok(Some(TempModel { _dir: dir, file }))
}

/// Parse `origin` into (host, owner/name).
fn origin() -> Result<(String, String)> {
    let url = git::output(&["remote", "get-url", "origin"]).context("this repository has no `origin` remote")?;
    parse_remote(&url).ok_or_else(|| anyhow!("origin ({url}) is not a ShapesDB URL"))
}

fn parse_remote(url: &str) -> Option<(String, String)> {
    let url = url.trim().trim_end_matches('/');
    let (host, path) = if let Some(rest) = url.strip_prefix("git@") {
        let (h, p) = rest.split_once(':')?;
        (format!("https://{h}"), p.to_string())
    } else {
        let parsed = url::Url::parse(url).ok()?;
        let host = format!("{}://{}{}", if parsed.scheme() == "ssh" { "https" } else { parsed.scheme() }, parsed.host_str()?, if parsed.scheme() == "ssh" { String::new() } else { parsed.port().map(|p| format!(":{p}")).unwrap_or_default() });
        (host, parsed.path().trim_start_matches('/').to_string())
    };
    let path = path.strip_suffix(".git").unwrap_or(&path).to_string();
    (path.split('/').count() == 2).then_some((host, path))
}

fn preview(path: Option<String>) -> Result<ExitCode> {
    let (host, repo) = origin()?;
    let url = match path {
        Some(p) => {
            let branch = git::output(&["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_else(|_| "main".into());
            format!("{host}/{repo}/blob/{branch}/{p}")
        }
        None => format!("{host}/{repo}"),
    };
    println!("{url}");
    let _ = open::that(&url);
    Ok(ExitCode::SUCCESS)
}

fn publish(cfg: &config::Config, host: &str, name: Option<String>, owner: Option<String>, private: bool, description: &str) -> Result<ExitCode> {
    if !git::in_repo() {
        bail!("run `shape init` first");
    }
    if git::output(&["remote", "get-url", "origin"]).is_ok() {
        bail!("this repository already has an `origin` remote; use `shape push`");
    }
    let name = match name {
        Some(n) => n,
        None => git::repo_root()?.file_name().and_then(|n| n.to_str()).map(String::from).context("cannot infer a repository name")?,
    };
    let api = client(cfg, host)?;
    let repo: api::Repo = api.post(
        "/repos",
        &json!({ "name": name, "owner": owner, "visibility": if private { "private" } else { "public" }, "description": description }),
    )?;
    git::run_ok(&["remote", "add", "origin", &repo.clone_http])?;
    let branch = git::output(&["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_else(|_| "main".into());
    let has_commits = git::output(&["rev-parse", "--verify", "HEAD"]).is_ok();
    if has_commits {
        git::run_ok(&["push", "-u", "origin", &branch])?;
    }
    println!("Published {host}/{} ({}).", repo.full_name, repo.visibility);
    Ok(ExitCode::SUCCESS)
}

fn release(cfg: &config::Config, host: &str, tag: &str, title: Option<String>, notes: &str, target: Option<String>, prerelease: bool) -> Result<ExitCode> {
    let (remote_host, repo) = origin()?;
    let host = if cfg.credentials(host).is_some() { host.to_string() } else { remote_host };
    let api = client(cfg, &host)?;
    let r: serde_json::Value = api.post(
        &format!("/repos/{repo}/releases"),
        &json!({ "tag_name": tag, "name": title.unwrap_or_default(), "body": notes, "target": target, "prerelease": prerelease }),
    )?;
    println!("Created release {} at {}.", r["tag_name"].as_str().unwrap_or(tag), r["target_commit"].as_str().unwrap_or("").get(..7).unwrap_or(""));
    println!("{host}/{repo}/releases");
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::parse_remote;

    #[test]
    fn remotes() {
        assert_eq!(parse_remote("https://shapesdb.com/alice/castle.git"), Some(("https://shapesdb.com".into(), "alice/castle".into())));
        assert_eq!(parse_remote("http://localhost:8080/alice/castle.git"), Some(("http://localhost:8080".into(), "alice/castle".into())));
        assert_eq!(parse_remote("git@git.shapesdb.com:alice/castle.git"), Some(("https://git.shapesdb.com".into(), "alice/castle".into())));
        assert_eq!(parse_remote("ssh://git@localhost:2222/alice/castle.git"), Some(("https://localhost".into(), "alice/castle".into())));
        assert_eq!(parse_remote("https://example.com/a/b/c"), None);
    }
}
