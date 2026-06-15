use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand};
use directories::ProjectDirs;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::process::Command;

const DEFAULT_BIND: &str = "127.0.0.1";
const DEFAULT_PROXY_PORT: u16 = 8080;
const PORT_START: u16 = 41000;
const PORT_END: u16 = 49999;

#[derive(Parser)]
#[command(
    name = "devhost",
    version,
    about = "Local hostnames for development app instances"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the local reverse proxy.
    Proxy {
        /// Address to bind the proxy to.
        #[arg(long, default_value = DEFAULT_BIND)]
        bind: IpAddr,
        /// Port to bind the proxy to.
        #[arg(long, short, default_value_t = DEFAULT_PROXY_PORT)]
        port: u16,
    },
    /// Run an app, allocate a port, and register a .localhost route.
    Run {
        /// Logical app name, e.g. web, api, admin.
        app: String,
        /// Exact host prefix override. `--host auth-v2` becomes auth-v2.localhost.
        #[arg(long)]
        host: Option<String>,
        /// Internal app port. If omitted, devhost chooses an available high port.
        #[arg(long)]
        port: Option<u16>,
        /// Command to run after `--`.
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },
    /// List known routes.
    Ps,
    /// Remove routes whose managed process is no longer alive.
    Clean,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct State {
    proxy: ProxyState,
    routes: BTreeMap<String, Route>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProxyState {
    host: String,
    port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Route {
    app: String,
    repo: String,
    context: String,
    cwd: PathBuf,
    target_host: String,
    target_port: u16,
    pid: Option<u32>,
    command: Vec<String>,
    started_at: DateTime<Utc>,
    managed: bool,
}

#[derive(Debug, Clone)]
struct Identity {
    repo: String,
    context: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Proxy { bind, port } => proxy(bind, port).await,
        Commands::Run {
            app,
            host,
            port,
            command,
        } => run(app, host, port, command).await,
        Commands::Ps => ps(),
        Commands::Clean => clean(),
    }
}

async fn proxy(bind: IpAddr, port: u16) -> Result<()> {
    let addr = SocketAddr::new(bind, port);
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind proxy on {addr}"))?;

    let mut state = load_state()?;
    state.proxy = ProxyState {
        host: bind.to_string(),
        port,
    };
    save_state(&state)?;

    println!("devhost proxy listening on http://{addr}");
    println!("routes: {}", state_path()?.display());

    loop {
        let (client, peer) = listener.accept().await?;
        tokio::spawn(async move {
            if let Err(err) = handle_connection(client).await {
                eprintln!("devhost proxy: {peer}: {err:#}");
            }
        });
    }
}

async fn handle_connection(mut client: TcpStream) -> Result<()> {
    let mut buf = Vec::with_capacity(8192);
    let mut tmp = [0_u8; 1024];

    loop {
        let n = client.read(&mut tmp).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&tmp[..n]);
        if headers_complete(&buf) {
            break;
        }
        if buf.len() > 64 * 1024 {
            bail!("request headers exceeded 64KiB");
        }
    }

    let host = parse_host(&buf).context("request did not include a Host header")?;
    let normalized = normalize_host(&host);
    let state = load_state()?;
    let route = match state.routes.get(&normalized) {
        Some(route) => route.clone(),
        None => {
            write_missing_route(&mut client, &normalized, &state).await?;
            return Ok(());
        }
    };

    let target_addr = format!("{}:{}", route.target_host, route.target_port);
    let mut target = TcpStream::connect(&target_addr)
        .await
        .with_context(|| format!("failed to connect to target {target_addr}"))?;

    target.write_all(&buf).await?;
    let _ = tokio::io::copy_bidirectional(&mut client, &mut target).await;
    Ok(())
}

fn headers_complete(buf: &[u8]) -> bool {
    buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.windows(2).any(|w| w == b"\n\n")
}

fn parse_host(buf: &[u8]) -> Option<String> {
    let request = String::from_utf8_lossy(buf);
    for line in request.lines() {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("host") {
            return Some(value.trim().to_string());
        }
    }
    None
}

fn normalize_host(host: &str) -> String {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if let Some(stripped) = host.strip_prefix('[') {
        if let Some((inside, _)) = stripped.split_once(']') {
            return inside.to_string();
        }
    }
    host.split(':').next().unwrap_or(&host).to_string()
}

async fn write_missing_route(client: &mut TcpStream, host: &str, state: &State) -> Result<()> {
    let mut body = format!("devhost: no route for {host}\n\nKnown routes:\n");
    if state.routes.is_empty() {
        body.push_str("  none\n");
    } else {
        for (name, route) in &state.routes {
            body.push_str(&format!(
                "  - {name} -> http://{}:{}\n",
                route.target_host, route.target_port
            ));
        }
    }
    let response = format!(
        "HTTP/1.1 404 Not Found\r\ncontent-type: text/plain; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        body.len(), body
    );
    client.write_all(response.as_bytes()).await?;
    Ok(())
}

async fn run(
    app: String,
    host: Option<String>,
    port: Option<u16>,
    command: Vec<String>,
) -> Result<()> {
    if command.is_empty() {
        bail!("missing command after --");
    }

    let cwd = std::env::current_dir().context("failed to read current directory")?;
    let identity = detect_identity(&cwd);
    let host_prefix = match host {
        Some(prefix) => slugify(&prefix),
        None => slugify(&format!("{}-{}-{}", app, identity.repo, identity.context)),
    };
    if host_prefix.is_empty() {
        bail!("hostname prefix is empty after slugification");
    }
    let hostname = format!("{host_prefix}.localhost");
    let app_port = match port {
        Some(port) => port,
        None => allocate_port()
            .await
            .context("failed to allocate an app port")?,
    };

    let mut state = load_state()?;
    let proxy = state.proxy.clone();
    let route = Route {
        app: app.clone(),
        repo: identity.repo.clone(),
        context: identity.context.clone(),
        cwd: cwd.clone(),
        target_host: DEFAULT_BIND.to_string(),
        target_port: app_port,
        pid: None,
        command: command.clone(),
        started_at: Utc::now(),
        managed: true,
    };
    state.routes.insert(hostname.clone(), route);
    save_state(&state)?;

    let mut child = Command::new(&command[0]);
    child
        .args(&command[1..])
        .current_dir(&cwd)
        .env("PORT", app_port.to_string())
        .env("HOST", DEFAULT_BIND)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let mut child = match child.spawn() {
        Ok(child) => child,
        Err(err) => {
            remove_route(&hostname)?;
            return Err(err).with_context(|| format!("failed to start command: {:?}", command));
        }
    };

    if let Some(pid) = child.id() {
        let mut state = load_state()?;
        if let Some(route) = state.routes.get_mut(&hostname) {
            route.pid = Some(pid);
        }
        save_state(&state)?;
    }

    println!("devhost");
    println!("  app:      {app}");
    println!("  repo:     {}", identity.repo);
    println!("  context:  {}", identity.context);
    println!("  url:      http://{}:{}", hostname, proxy.port);
    println!("  target:   http://{}:{}", DEFAULT_BIND, app_port);
    println!("  command:  {}", command.join(" "));
    println!();

    let status = tokio::select! {
        status = child.wait() => status.context("failed while waiting for child")?,
        _ = tokio::signal::ctrl_c() => {
            let _ = child.kill().await;
            child.wait().await.context("failed while waiting for child after Ctrl-C")?
        }
    };

    remove_route(&hostname)?;
    if !status.success() {
        match status.code() {
            Some(code) => bail!("child exited with status {code}"),
            None => bail!("child terminated by signal"),
        }
    }
    Ok(())
}

fn ps() -> Result<()> {
    let state = load_state()?;
    if state.routes.is_empty() {
        println!("No devhost routes.");
        return Ok(());
    }

    println!(
        "{:<18} {:<18} {:<44} {:<22} {}",
        "APP", "CONTEXT", "URL", "TARGET", "PID"
    );
    for (host, route) in state.routes {
        let pid = route
            .pid
            .map(|pid| pid.to_string())
            .unwrap_or_else(|| "-".to_string());
        println!(
            "{:<18} {:<18} {:<44} http://{}:{:<6} {}",
            route.app,
            route.context,
            format!("http://{}:{}", host, state.proxy.port),
            route.target_host,
            route.target_port,
            pid
        );
    }
    Ok(())
}

fn clean() -> Result<()> {
    let mut state = load_state()?;
    let before = state.routes.len();
    state.routes.retain(|_, route| match route.pid {
        Some(pid) if route.managed => pid_is_alive(pid),
        _ => true,
    });
    let removed = before - state.routes.len();
    save_state(&state)?;
    if removed == 0 {
        println!("Nothing to clean.");
    } else {
        println!(
            "Removed {removed} stale route{}.",
            if removed == 1 { "" } else { "s" }
        );
    }
    Ok(())
}

fn pid_is_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        std::process::Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true
    }
}

async fn allocate_port() -> Result<u16> {
    let mut rng = rand::rng();
    for _ in 0..200 {
        let port = rng.random_range(PORT_START..=PORT_END);
        if port_available(port).await {
            return Ok(port);
        }
    }
    for port in PORT_START..=PORT_END {
        if port_available(port).await {
            return Ok(port);
        }
    }
    bail!("no available port in {PORT_START}-{PORT_END}")
}

async fn port_available(port: u16) -> bool {
    TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await.is_ok()
}

fn detect_identity(cwd: &Path) -> Identity {
    let git_root = git(cwd, &["rev-parse", "--show-toplevel"]).ok();
    let repo = git(cwd, &["config", "--get", "remote.origin.url"])
        .ok()
        .and_then(|url| repo_name_from_remote(&url))
        .or_else(|| {
            git_root
                .as_deref()
                .and_then(|root| Path::new(root).file_name())
                .map(|s| s.to_string_lossy().to_string())
        })
        .or_else(|| cwd.file_name().map(|s| s.to_string_lossy().to_string()))
        .unwrap_or_else(|| "app".to_string());

    let context = git(cwd, &["branch", "--show-current"])
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            git(cwd, &["rev-parse", "--short", "HEAD"])
                .ok()
                .map(|s| format!("detached-{s}"))
        })
        .or_else(|| cwd.file_name().map(|s| s.to_string_lossy().to_string()))
        .unwrap_or_else(|| "local".to_string());

    Identity {
        repo: slugify(&repo),
        context: slugify(&context),
    }
}

fn git(cwd: &Path, args: &[&str]) -> Result<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;
    if !output.status.success() {
        bail!("git {} failed", args.join(" "));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn repo_name_from_remote(url: &str) -> Option<String> {
    let trimmed = url.trim().trim_end_matches('/');
    let last = trimmed.rsplit(['/', ':']).next()?;
    let name = last.strip_suffix(".git").unwrap_or(last);
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn slugify(input: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in input.chars().flat_map(|ch| ch.to_lowercase()) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.len() <= 63 {
        trimmed
    } else {
        trimmed[..63].trim_end_matches('-').to_string()
    }
}

fn state_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("DEVHOST_STATE") {
        return Ok(PathBuf::from(path));
    }
    let dirs = ProjectDirs::from("dev", "atelier", "devhost")
        .context("failed to determine config directory")?;
    Ok(dirs.config_dir().join("state.json"))
}

fn load_state() -> Result<State> {
    let path = state_path()?;
    if !path.exists() {
        return Ok(default_state());
    }
    let bytes = fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
    if bytes.is_empty() {
        return Ok(default_state());
    }
    serde_json::from_slice(&bytes).with_context(|| format!("failed to parse {}", path.display()))
}

fn save_state(state: &State) -> Result<()> {
    let path = state_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(state)?;
    fs::write(&tmp, bytes).with_context(|| format!("failed to write {}", tmp.display()))?;
    fs::rename(&tmp, &path).with_context(|| {
        format!(
            "failed to atomically replace {} with {}",
            path.display(),
            tmp.display()
        )
    })?;
    Ok(())
}

fn default_state() -> State {
    State {
        proxy: ProxyState {
            host: DEFAULT_BIND.to_string(),
            port: DEFAULT_PROXY_PORT,
        },
        routes: BTreeMap::new(),
    }
}

fn remove_route(hostname: &str) -> Result<()> {
    let mut state = load_state()?;
    state.routes.remove(hostname);
    save_state(&state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugifies_branch_names() {
        assert_eq!(slugify("Feature/auth:Callbacks"), "feature-auth-callbacks");
        assert_eq!(slugify("---A__B---"), "a-b");
    }

    #[test]
    fn parses_repo_names_from_remotes() {
        assert_eq!(
            repo_name_from_remote("git@github.com:org/atelier.git").as_deref(),
            Some("atelier")
        );
        assert_eq!(
            repo_name_from_remote("https://github.com/org/atelier.git").as_deref(),
            Some("atelier")
        );
    }

    #[test]
    fn normalizes_host_header() {
        assert_eq!(normalize_host("web.localhost:8080"), "web.localhost");
        assert_eq!(normalize_host("WEB.localhost."), "web.localhost");
    }

    #[test]
    fn parses_host_after_request_line() {
        let request = b"GET / HTTP/1.1\r\nHost: web.localhost:8080\r\n\r\n";
        assert_eq!(parse_host(request).as_deref(), Some("web.localhost:8080"));
    }
}
