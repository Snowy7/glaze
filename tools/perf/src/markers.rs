//! The startup markers components log (crates/desktop/src/startup.rs):
//!
//! ```text
//! [2026-11-04T10:00:00Z INFO  glaze_desktop::startup] READY component=wallpaper first_frame_ms=84.2 epoch_us=1793612345678901
//! ```

/// One `READY` line.
#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    pub component: String,
    /// The metric's name without its `_ms` suffix, e.g. `first_frame`.
    pub metric: String,
    /// Milliseconds since the component's own process was created.
    pub own_ms: f64,
    /// When it happened, in Unix microseconds.
    pub epoch_us: u64,
}

/// Parses a log line; `None` unless it's a complete `READY` marker.
pub fn parse(line: &str) -> Option<Marker> {
    let rest = &line[line.find("READY ")? + "READY ".len()..];
    let (mut component, mut metric, mut epoch_us) = (None, None, None);
    for token in rest.split_whitespace() {
        let Some((key, value)) = token.split_once('=') else {
            continue;
        };
        match key {
            "component" => component = Some(value),
            "epoch_us" => epoch_us = value.parse().ok(),
            _ => {
                if let Some(name) = key.strip_suffix("_ms") {
                    metric = Some((name, value.parse().ok()?));
                }
            }
        }
    }
    let (metric, own_ms) = metric?;
    Some(Marker {
        component: component?.to_owned(),
        metric: metric.to_owned(),
        own_ms,
        epoch_us: epoch_us?,
    })
}

/// Every marker in a log excerpt, in order.
pub fn parse_all(text: &str) -> Vec<Marker> {
    text.lines().filter_map(parse).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_marker_inside_an_env_logger_line() {
        let line = "[2026-11-04T10:00:00Z INFO  glaze_desktop::startup] READY \
                    component=wallpaper first_frame_ms=84.2 epoch_us=1793612345678901";
        assert_eq!(
            parse(line),
            Some(Marker {
                component: "wallpaper".into(),
                metric: "first_frame".into(),
                own_ms: 84.2,
                epoch_us: 1_793_612_345_678_901,
            })
        );
    }

    #[test]
    fn ignores_other_lines_and_incomplete_markers() {
        assert_eq!(parse("[.. INFO glaze_core] running"), None);
        assert_eq!(parse("READY component=bars first_paint_ms=12"), None);
        assert_eq!(parse("READY first_paint_ms=12 epoch_us=5"), None);
        assert_eq!(
            parse("READY component=bars first_paint_ms=soon epoch_us=5"),
            None
        );
    }

    #[test]
    fn collects_every_marker_of_an_excerpt() {
        let log = "a\nREADY component=core spawn_ms=3.5 epoch_us=10\nb\n\
                   READY component=bars first_paint_ms=400.0 epoch_us=20\n";
        let found: Vec<_> = parse_all(log).into_iter().map(|m| m.metric).collect();
        assert_eq!(found, ["spawn", "first_paint"]);
    }
}
