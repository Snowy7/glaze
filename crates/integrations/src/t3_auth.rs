//! Background T3 Code access: the local `t3` CLI issues mypaper a scoped bearer as the same
//! Windows user, with no prompt. The token lives only in Credential Manager (`t3`); its session
//! id and expiry sit beside it (`t3-session`) so it is renewed early and revoked when replaced.
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

/// Exactly what the widgets use: thread status, usage and limits, and thread controls.
pub const SCOPES: [&str; 3] = [
    "orchestration:read",
    "orchestration:operate",
    "diagnostics:read",
];
const LABEL: &str = "mypaper";
/// Renew once less than this is left of the 30-day grant.
const RENEW_BEFORE_DAYS: i64 = 3;

#[derive(Serialize, Deserialize)]
struct Session {
    id: String,
    expires_at: String,
}

fn userdata() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("USERPROFILE")?).join(".t3").join("userdata"))
}

/// The local T3 server's port, when `server-runtime.json` names one that accepts connections.
pub fn running_port() -> Option<u16> {
    #[derive(Deserialize)]
    struct Runtime {
        port: u16,
    }
    let bytes = std::fs::read(userdata()?.join("server-runtime.json")).ok()?;
    let port = serde_json::from_slice::<Runtime>(&bytes).ok()?.port;
    std::net::TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_secs(1))
        .ok()
        .map(|_| port)
}

/// T3 records the directory holding its `t3.cmd` launcher.
fn cli() -> Option<PathBuf> {
    let recorded = std::fs::read_to_string(userdata()?.join("cli-command-path-entry")).ok();
    let dir = recorded
        .map(|s| PathBuf::from(s.trim()))
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| Some(PathBuf::from(std::env::var_os("USERPROFILE")?).join(".t3").join("bin")))?;
    Some(dir.join("t3.cmd")).filter(|p| p.is_file())
}

/// One hidden CLI run, killed after `timeout`. Stdout only; stderr is Electron noise.
fn run(args: &[&str], timeout: Duration) -> Result<Vec<u8>> {
    use std::io::Read;
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let cli = cli().context("T3 Code's command line tool was not found")?;
    let mut child = std::process::Command::new(cli)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("T3 Code's command line tool did not start")?;
    let mut stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout.by_ref().take(64 * 1024).read_to_end(&mut out);
        out
    });
    let started = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            bail!("T3 Code's command line tool timed out");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let out = reader.join().unwrap_or_default();
    ensure!(status.success(), "T3 Code's command line tool failed");
    Ok(out)
}

fn session() -> Option<Session> {
    let secret = crate::credentials::read("t3-session").ok()?;
    serde_json::from_str(secret.text().ok()?).ok()
}

fn fresh(session: &Session) -> bool {
    chrono::DateTime::parse_from_rfc3339(&session.expires_at)
        .is_ok_and(|at| at.to_utc() - chrono::Utc::now() > chrono::TimeDelta::days(RENEW_BEFORE_DAYS))
}

/// True when the local server and CLI can mint access, so no pairing UI is needed.
pub fn available() -> bool {
    running_port().is_some() && cli().is_some()
}

/// Makes sure Credential Manager holds a current CLI-issued token. `force` replaces a token the
/// server rejected. Returns whether a new token was issued. A token from an older manual pairing
/// (no session record) is replaced too, so the widgets always hold the full scope set.
pub fn ensure(force: bool) -> Result<bool> {
    let previous = session();
    if !force
        && crate::credentials::read("t3").is_ok()
        && previous.as_ref().is_some_and(fresh)
    {
        return Ok(false);
    }
    ensure!(running_port().is_some(), "T3 Code is not running");
    let mut args = vec!["auth", "session", "issue"];
    for scope in SCOPES {
        args.extend(["--scope", scope]);
    }
    args.extend(["--label", LABEL, "--ttl", "30d", "--json"]);
    let mut out = run(&args, Duration::from_secs(20))?;
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Issued {
        session_id: String,
        token: String,
        expires_at: String,
    }
    let parsed = serde_json::from_slice::<Issued>(&out);
    out.fill(0);
    let Issued {
        session_id,
        mut token,
        expires_at,
    } = parsed.map_err(|_| anyhow::anyhow!("T3 Code returned an unexpected session"))?;
    let saved = crate::credentials::write("t3", &token);
    // SAFETY: zeroing ASCII bytes keeps the String valid UTF-8.
    unsafe { token.as_bytes_mut().fill(0) };
    saved?;
    crate::credentials::write(
        "t3-session",
        &serde_json::to_string(&Session {
            id: session_id.clone(),
            expires_at,
        })?,
    )?;
    log::info!("T3: issued mypaper access session {session_id}");
    revoke_others(&session_id, previous.map(|s| s.id));
    Ok(true)
}

/// Keeps T3's connections list clean: the replaced session, and any other `mypaper` session.
fn revoke_others(current: &str, previous: Option<String>) {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Listed {
        session_id: String,
        #[serde(default)]
        client: Client,
    }
    #[derive(Default, Deserialize)]
    struct Client {
        label: Option<String>,
    }
    let mut stale: Vec<String> = previous.into_iter().collect();
    if let Ok(out) = run(&["auth", "session", "list", "--json"], Duration::from_secs(15))
        && let Ok(list) = serde_json::from_slice::<Vec<Listed>>(&out)
    {
        stale.extend(
            list.into_iter()
                .filter(|s| s.client.label.as_deref() == Some(LABEL))
                .map(|s| s.session_id),
        );
    }
    stale.sort();
    stale.dedup();
    for id in stale.iter().filter(|id| *id != current) {
        match run(&["auth", "session", "revoke", id], Duration::from_secs(15)) {
            Ok(_) => log::info!("T3: revoked replaced session {id}"),
            Err(error) => log::warn!("T3: could not revoke session {id}: {error:#}"),
        }
    }
}
