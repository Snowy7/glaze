//! The network rule for widget frames (docs/widgets.md, "Network").
//!
//! Pure: the WebView's `WebResourceRequested` handler hands us the request
//! URL and its `Origin`/`Referer` headers, and [`decide`] says yes or no.
//! A request is attributed to the frame that made it by those headers, which
//! Chromium sets from the requesting document and pages can't forge. Anything
//! that can't be attributed is blocked.

use std::fmt;

/// The shell's virtual host.
pub const SHELL_HOST: &str = "shell.mypaper";
/// Widget virtual hosts are `<id>.widget.mypaper`.
pub const WIDGET_SUFFIX: &str = ".widget.mypaper";

/// A web origin, `scheme://host[:port]`. The port is `None` when it's the
/// scheme's default, so `https://a.b` and `https://a.b:443` compare equal.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Origin {
    scheme: String,
    host: String,
    port: Option<u16>,
}

impl Origin {
    /// Parses a bare origin, as written in a `network:<origin>` permission:
    /// no path, query, fragment or user info (a trailing `/` is tolerated).
    pub fn parse(text: &str) -> Option<Self> {
        let rest = text.strip_suffix('/').unwrap_or(text);
        let (_, after) = rest.split_once("://")?;
        if after.contains(['/', '?', '#', '@']) {
            return None;
        }
        Self::of_url(rest)
    }

    /// The origin of an `http`, `https`, `ws` or `wss` URL.
    pub fn of_url(url: &str) -> Option<Self> {
        let (scheme, rest) = url.split_once("://")?;
        let scheme = scheme.to_ascii_lowercase();
        let default_port = match scheme.as_str() {
            "http" | "ws" => 80,
            "https" | "wss" => 443,
            _ => return None,
        };
        let authority = rest.split(['/', '?', '#']).next()?;
        let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        let (host, port) = match host_port.strip_prefix('[') {
            // [v6]:port
            Some(v6) => {
                let (inside, after) = v6.split_once(']')?;
                let port = match after.strip_prefix(':') {
                    Some(p) => Some(p.parse::<u16>().ok()?),
                    None if after.is_empty() => None,
                    None => return None,
                };
                (format!("[{inside}]"), port)
            }
            None => match host_port.rsplit_once(':') {
                Some((host, p)) => (host.to_owned(), Some(p.parse::<u16>().ok()?)),
                None => (host_port.to_owned(), None),
            },
        };
        if host.is_empty() || host.contains(char::is_whitespace) {
            return None;
        }
        Some(Self {
            scheme,
            host: host.to_ascii_lowercase(),
            port: port.filter(|&p| p != default_port),
        })
    }

    /// Whose origin this is: the shell, a widget, or the outside world.
    pub fn party(&self) -> Party<'_> {
        if self.scheme != "https" || self.port.is_some() {
            return Party::Outside;
        }
        if self.host == SHELL_HOST {
            return Party::Shell;
        }
        match self.host.strip_suffix(WIDGET_SUFFIX) {
            Some(id) if crate::manifest::valid_id(id) => Party::Widget(id),
            _ => Party::Outside,
        }
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}://{}", self.scheme, self.host)?;
        match self.port {
            Some(port) => write!(f, ":{port}"),
            None => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Party<'a> {
    Shell,
    Widget(&'a str),
    Outside,
}

/// Who made a request, from its headers: the `Origin` header when it's a
/// real origin, else the `Referer`'s origin. A `null` origin, or neither
/// header, is `None` (unattributable).
pub fn initiator(origin: Option<&str>, referer: Option<&str>) -> Option<Origin> {
    match origin.map(str::trim) {
        Some("null") => None,
        Some(origin) if !origin.is_empty() => Origin::parse(origin),
        _ => referer.and_then(Origin::of_url),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Block,
}

/// The network rule. `allowed(widget, origin)` says whether that widget's
/// manifest lists `network:<origin>`. `service_worker` is set for requests
/// carrying the `Service-Worker` header (service-worker scripts).
pub fn decide(
    url: &str,
    initiator: Option<&Origin>,
    service_worker: bool,
    allowed: impl Fn(&str, &Origin) -> bool,
) -> Verdict {
    if ["data:", "blob:", "about:"].iter().any(|scheme| {
        url.get(..scheme.len())
            .is_some_and(|s| s.eq_ignore_ascii_case(scheme))
    }) {
        return Verdict::Allow; // never reaches the network
    }
    let Some(target) = Origin::of_url(url) else {
        return Verdict::Block; // file:, ftp:, edge:, …
    };
    if service_worker {
        return Verdict::Block;
    }
    let from = initiator.map(Origin::party);
    let ok = match target.party() {
        Party::Shell => matches!(from, None | Some(Party::Shell)),
        Party::Widget(id) => match from {
            Some(Party::Shell) => true,
            Some(Party::Widget(by)) => by == id,
            _ => false,
        },
        Party::Outside => matches!(from, Some(Party::Widget(by)) if allowed(by, &target)),
    };
    if ok { Verdict::Allow } else { Verdict::Block }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin(text: &str) -> Origin {
        Origin::parse(text).unwrap()
    }

    #[test]
    fn origins_normalize_case_and_default_ports() {
        assert_eq!(
            origin("HTTPS://API.Open-Meteo.com"),
            origin("https://api.open-meteo.com:443")
        );
        assert_eq!(origin("https://a.com/"), origin("https://a.com"));
        assert_ne!(origin("http://a.com"), origin("https://a.com"));
        assert_ne!(origin("https://a.com:8443"), origin("https://a.com"));
        assert_eq!(
            origin("http://localhost:8080").to_string(),
            "http://localhost:8080"
        );
        assert_eq!(origin("wss://[::1]:9000").to_string(), "wss://[::1]:9000");
    }

    #[test]
    fn permission_origins_reject_paths_and_junk() {
        for bad in [
            "https://a.com/path",
            "https://a.com?x",
            "https://u@a.com",
            "ftp://a.com",
            "a.com",
            "https://",
            "https://a.com:99999",
        ] {
            assert_eq!(Origin::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn url_origins_ignore_path_query_and_user_info() {
        let url = "https://user:pw@Api.Open-Meteo.com:443/v1/forecast?lat=1#x";
        assert_eq!(
            Origin::of_url(url),
            Some(origin("https://api.open-meteo.com"))
        );
    }

    #[test]
    fn parties() {
        assert_eq!(origin("https://shell.mypaper").party(), Party::Shell);
        assert_eq!(
            origin("https://clock.widget.mypaper").party(),
            Party::Widget("clock")
        );
        assert_eq!(
            origin("http://clock.widget.mypaper").party(),
            Party::Outside
        );
        assert_eq!(origin("https://a.b.widget.mypaper").party(), Party::Outside);
        assert_eq!(origin("https://evil.com").party(), Party::Outside);
    }

    #[test]
    fn initiator_prefers_origin_then_referer() {
        let weather = Some(origin("https://weather.widget.mypaper"));
        assert_eq!(
            initiator(Some("https://weather.widget.mypaper"), None),
            weather
        );
        let referer = "https://weather.widget.mypaper/index.html";
        assert_eq!(initiator(None, Some(referer)), weather);
        assert_eq!(initiator(Some("null"), Some(referer)), None);
        assert_eq!(initiator(None, None), None);
    }

    const METEO: &str = "https://api.open-meteo.com/v1/forecast";

    fn weather_only(widget: &str, target: &Origin) -> bool {
        widget == "weather" && *target == origin("https://api.open-meteo.com")
    }

    fn check(url: &str, from: Option<&str>) -> Verdict {
        decide(url, from.map(origin).as_ref(), false, weather_only)
    }

    #[test]
    fn widgets_reach_only_their_declared_origins() {
        assert_eq!(
            check(METEO, Some("https://weather.widget.mypaper")),
            Verdict::Allow
        );
        assert_eq!(
            check(METEO, Some("https://clock.widget.mypaper")),
            Verdict::Block
        );
        assert_eq!(
            check("https://evil.com/x", Some("https://weather.widget.mypaper")),
            Verdict::Block
        );
        // Unattributable, or from the shell: never the outside world.
        assert_eq!(check(METEO, None), Verdict::Block);
        assert_eq!(check(METEO, Some("https://shell.mypaper")), Verdict::Block);
        // Scheme matters: a ws origin isn't the https one.
        assert_eq!(
            check(
                "wss://api.open-meteo.com/",
                Some("https://weather.widget.mypaper")
            ),
            Verdict::Block
        );
    }

    #[test]
    fn widget_hosts_load_from_the_shell_or_themselves() {
        let clock = "https://clock.widget.mypaper/index.html";
        assert_eq!(check(clock, Some("https://shell.mypaper")), Verdict::Allow);
        assert_eq!(
            check(clock, Some("https://clock.widget.mypaper")),
            Verdict::Allow
        );
        assert_eq!(
            check(clock, Some("https://weather.widget.mypaper")),
            Verdict::Block
        );
        assert_eq!(check(clock, None), Verdict::Block);
    }

    #[test]
    fn the_shell_is_only_for_the_shell() {
        let shell = "https://shell.mypaper/assets/index.js";
        assert_eq!(check(shell, None), Verdict::Allow);
        assert_eq!(check(shell, Some("https://shell.mypaper")), Verdict::Allow);
        assert_eq!(
            check(shell, Some("https://clock.widget.mypaper")),
            Verdict::Block
        );
    }

    #[test]
    fn local_schemes_pass_and_others_block() {
        assert_eq!(check("data:text/plain,hi", None), Verdict::Allow);
        assert_eq!(
            check("blob:https://clock.widget.mypaper/1", None),
            Verdict::Allow
        );
        assert_eq!(
            check(
                "file:///C:/Windows/win.ini",
                Some("https://clock.widget.mypaper")
            ),
            Verdict::Block
        );
    }

    #[test]
    fn service_worker_scripts_block() {
        let from = origin("https://weather.widget.mypaper");
        assert_eq!(
            decide(
                "https://weather.widget.mypaper/sw.js",
                Some(&from),
                true,
                weather_only
            ),
            Verdict::Block
        );
    }
}
