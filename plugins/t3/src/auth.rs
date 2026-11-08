//! T3's background key: the local `t3` CLI issues Glaze a scoped bearer as the same Windows
//! user, with no prompt (`process.run`). The token lives only in the secrets broker (`t3`); its
//! session id and expiry sit beside it (`t3-session`), so it is renewed early and the replaced
//! session, and any other `Glaze` session, is revoked.
use glaze_plugin_sdk::guest::{self, files, process, secrets};
use serde::{Deserialize, Serialize};

/// Exactly what the faces use: thread status, usage and limits, and thread controls.
const SCOPES: [&str; 3] = ["orchestration:read", "orchestration:operate", "diagnostics:read"];
const LABEL: &str = "Glaze";
/// Renew once less than this is left of the 30-day grant.
const RENEW_BEFORE_DAYS: i64 = 3;

#[derive(Serialize, Deserialize)]
struct Session {
    id: String,
    expires_at: String,
}

#[derive(Default)]
enum Step {
    #[default]
    Idle,
    Issuing(u64),
    Listing(u64, String, Option<String>),
}

/// What [`Auth::reply`] decided.
pub enum Outcome {
    /// Not this module's reply.
    Other,
    Pending,
    Ready(String),
    Failed(String),
}

#[derive(Default)]
pub struct Auth {
    step: Step,
    revoking: Vec<u64>,
}

/// T3 records the directory holding its `t3.cmd` launcher; else `t3` on PATH.
fn cli() -> String {
    let recorded = files::read("~/.t3/userdata/cli-command-path-entry").ok().flatten();
    recorded
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map(|dir| dir.trim().trim_end_matches('\\').to_owned())
        .filter(|dir| !dir.is_empty())
        .map_or_else(|| "t3".to_owned(), |dir| format!("{dir}\\t3.cmd"))
}

fn run(args: &[&str]) -> Result<u64, String> {
    let request = process::RunRequest { exe: cli(), args: args.iter().map(|a| (*a).to_owned()).collect(), timeout_ms: Some(20_000) };
    process::run(&request).map_err(|e| format!("T3 Code's command line tool didn't run ({e:?})"))
}

fn session() -> Option<Session> {
    serde_json::from_str(&secrets::get("t3-session").ok()??).ok()
}

fn fresh(session: &Session) -> bool {
    chrono::DateTime::parse_from_rfc3339(&session.expires_at).is_ok_and(|at| at.to_utc() - chrono::Utc::now() > chrono::TimeDelta::days(RENEW_BEFORE_DAYS))
}

impl Auth {
    pub fn busy(&self) -> bool {
        !matches!(self.step, Step::Idle)
    }

    /// A current token right away, or starts issuing one (`force` replaces a refused token).
    pub fn ensure(&mut self, force: bool) -> Outcome {
        if self.busy() {
            return Outcome::Pending;
        }
        let token = secrets::get("t3").ok().flatten();
        if !force && let Some(token) = token.filter(|_| session().as_ref().is_some_and(fresh)) {
            return Outcome::Ready(token);
        }
        let mut args = vec!["auth", "session", "issue"];
        for scope in SCOPES {
            args.extend(["--scope", scope]);
        }
        args.extend(["--label", LABEL, "--ttl", "30d", "--json"]);
        match run(&args) {
            Ok(id) => {
                self.step = Step::Issuing(id);
                Outcome::Pending
            }
            Err(e) => Outcome::Failed(e),
        }
    }

    /// A `process.run` result (`event::http`): status is the exit code, body stdout.
    pub fn reply(&mut self, id: u64, outcome: &Result<(u16, Vec<u8>), String>) -> Outcome {
        if let Some(at) = self.revoking.iter().position(|&r| r == id) {
            self.revoking.swap_remove(at);
            if !matches!(outcome, Ok((0, _))) {
                guest::log("T3: a stale Glaze session could not be revoked");
            }
            return Outcome::Pending;
        }
        match std::mem::take(&mut self.step) {
            Step::Issuing(issuing) if issuing == id => self.issued(outcome),
            Step::Listing(listing, current, previous) if listing == id => {
                self.revoke(outcome, &current, previous);
                Outcome::Pending
            }
            step => {
                self.step = step;
                Outcome::Other
            }
        }
    }

    fn issued(&mut self, outcome: &Result<(u16, Vec<u8>), String>) -> Outcome {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Issued {
            session_id: String,
            token: String,
            expires_at: String,
        }
        let issued = match outcome {
            Ok((0, out)) => serde_json::from_slice::<Issued>(out).map_err(|_| "T3 Code returned an unexpected session".to_owned()),
            Ok((code, _)) => Err(format!("T3 Code's command line tool failed ({code}); is T3 Code running?")),
            Err(e) => Err(e.clone()),
        };
        let Issued { session_id, token, expires_at } = match issued {
            Ok(issued) => issued,
            Err(e) => return Outcome::Failed(e),
        };
        let previous = session().map(|s| s.id);
        let record = serde_json::to_string(&Session { id: session_id.clone(), expires_at }).unwrap_or_default();
        if let Err(e) = secrets::set("t3", &token).and_then(|()| secrets::set("t3-session", &record)) {
            return Outcome::Failed(format!("Couldn't keep T3's key ({e:?})"));
        }
        guest::log(&format!("T3: issued Glaze access session {session_id}"));
        if let Ok(id) = run(&["auth", "session", "list", "--json"]) {
            self.step = Step::Listing(id, session_id, previous);
        }
        Outcome::Ready(token)
    }

    /// Keeps T3's connections list clean: the replaced session, and any other `Glaze` one.
    fn revoke(&mut self, outcome: &Result<(u16, Vec<u8>), String>, current: &str, previous: Option<String>) {
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
        if let Ok((0, out)) = outcome
            && let Ok(list) = serde_json::from_slice::<Vec<Listed>>(out)
        {
            stale.extend(list.into_iter().filter(|s| s.client.label.as_deref() == Some(LABEL)).map(|s| s.session_id));
        }
        stale.sort();
        stale.dedup();
        for id in stale.iter().filter(|id| *id != current) {
            if let Ok(run) = run(&["auth", "session", "revoke", id]) {
                self.revoking.push(run);
            }
        }
    }
}
