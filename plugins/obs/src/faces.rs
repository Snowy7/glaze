use crate::{Availability, Data, Snapshot, obs};
use mypaper_plugin_sdk::{Face, Role, SizeClass, Tone, View};

fn duration(ms: u64) -> String {
    let seconds = ms / 1000;
    format!("{:02}:{:02}:{:02}", seconds / 3600, seconds / 60 % 60, seconds % 60)
}
fn stack(children: Vec<View>, gap: f32) -> View { View::Stack { children, gap: Some(gap) } }
fn timer(label: &str, clock: &obs::Clock, active: bool, at: i64) -> View {
    stack(vec![
        View::tinted(label, Role::Caption, if active { Tone::Destructive } else { Tone::Tertiary }),
        View::text(duration(clock.elapsed(at)), Role::Title1),
    ], 1.0)
}
fn controls(state: &obs::Snapshot) -> View {
    View::row([
        View::button(if state.recording { "Stop" } else { "Record" }, if state.recording { "record.stop" } else { "record.start" }),
        View::button(if state.streaming { "End" } else { "Stream" }, if state.streaming { "stream.stop" } else { "stream.start" }),
    ])
}
fn stats(state: &obs::Stats) -> String {
    let cpu = state.cpu_tenths.map(|v| format!("{}.{:01}% CPU", v / 10, v % 10));
    let bitrate = state.bitrate_kbps.map(|v| format!("{v} kbps"));
    let dropped = state.dropped_frames.map(|v| format!("{v} dropped"));
    [cpu, bitrate, dropped].into_iter().flatten().collect::<Vec<_>>().join(" · ")
}
fn scenes(state: &obs::Snapshot) -> View {
    View::Select { label: "Scene".into(), action: "scene.select".into(),
        options: state.scenes.iter().map(|name| mypaper_plugin_sdk::view::SelectOption { label: name.clone(), value: name.clone() }).collect(),
        selected: Some(state.scene.clone()) }
}

/// Three bodies, using the native fixture's clocks, scene list and microphone controls.
pub fn face(size: SizeClass, snapshot: &Snapshot, now: i64) -> Face {
    let live = snapshot.availability == Availability::Live;
    let Some(Data::Obs(state)) = &snapshot.data else {
        let detail = match &snapshot.availability {
            Availability::Unavailable(why) | Availability::Stale(why) => why.as_str(),
            _ => "Enable OBS WebSocket; save its password in Settings.",
        };
        let title = if detail == "OBS isn't running" { "OBS isn't running" } else if snapshot.availability == Availability::Connecting { "Connecting to OBS…" } else { "Connect OBS" };
        let detail = if detail == "OBS isn't running" { "Open OBS and enable its WebSocket server." } else { detail };
        let mut face = View::Empty { icon: Some("video".into()), title: title.into(), detail: (size != SizeClass::Small).then(|| detail.into()) }.face();
        face.refresh = true;
        return face;
    };
    let at = if live { now } else { snapshot.observed_at_ms };
    let label = if state.paused { "PAUSED" } else if state.recording { "REC" } else if state.streaming { "LIVE" } else { "STANDBY" };
    let clock = if !state.recording && state.streaming { &state.stream_clock } else { &state.record_clock };
    let mut children = match size {
        SizeClass::Small => vec![timer(label, clock, state.recording || state.streaming, at)],
        SizeClass::Medium => vec![View::row([
            timer(label, clock, state.recording || state.streaming, at),
            stack(vec![scenes(state),
                View::tinted(if state.streaming { format!("Live · {}", duration(state.stream_clock.elapsed(at))) } else { "Stream offline".into() }, Role::Caption, Tone::Secondary)], 2.0),
        ])],
        SizeClass::Large => vec![View::row([
            timer(if state.paused { "PAUSED" } else { "RECORDING" }, &state.record_clock, state.recording, at),
            timer("STREAMING", &state.stream_clock, state.streaming, at),
        ])],
    };
    // Keep controls first so they remain available even in the smallest supported tile.
    if live { children.push(controls(state)); }
    if size != SizeClass::Small {
        if let Some(input) = state.microphones.first() && live {
            let mut buttons = vec![View::button(if input.muted { "Mic muted" } else { "Mic on" }, "mic:0")];
            if state.recording {
                buttons.push(View::button(if state.paused { "Resume" } else { "Pause" }, if state.paused { "record.resume" } else { "record.pause" }));
            }
            children.push(View::row(buttons));
        } else if state.recording && live {
            children.push(View::button(if state.paused { "Resume" } else { "Pause" }, if state.paused { "record.resume" } else { "record.pause" }));
        }
    }
    if size == SizeClass::Large {
        if live { children.push(scenes(state)); }
        for (index, name) in state.scenes.iter().take(2).enumerate() {
            children.push(View::ListRow { title: name.clone(), detail: None, icon: Some("layers".into()),
                trailing: (name == &state.scene).then(|| "✓".into()),
                action: live.then(|| format!("scene:{index}")) });
        }
    }
    let mut face = stack(children, if size == SizeClass::Small { 2.0 } else { 6.0 }).face();
    face.refresh = true;
    face.footer = Some(match &snapshot.availability {
        Availability::Stale(why) | Availability::Unavailable(why) => format!("Disconnected · {why}"),
        _ => { let summary = stats(&state.stats); if summary.is_empty() { state.scene.clone() } else { summary } },
    });
    face
}

/// Resolve only actions advertised by the current state; stale scene/mic indices do nothing.
pub fn action(action: &str, data: Option<&Data>) -> Option<obs::Action> {
    use obs::Action::*;
    let Some(Data::Obs(state)) = data else { return None; };
    match action {
        "record.start" if !state.recording => Some(StartRecording),
        "record.stop" if state.recording => Some(StopRecording),
        "record.pause" if state.recording && !state.paused => Some(PauseRecording),
        "record.resume" if state.recording && state.paused => Some(ResumeRecording),
        "stream.start" if !state.streaming => Some(StartStreaming),
        "stream.stop" if state.streaming => Some(StopStreaming),
        _ => match action.split_once(':') {
            Some(("scene", index)) => state.scenes.get(index.parse::<usize>().ok()?).cloned().map(SwitchScene),
            Some(("mic", index)) => state.microphones.get(index.parse::<usize>().ok()?).map(|input| MuteInput { name: input.name.clone(), muted: !input.muted }),
            _ => None,
        }
    }
}
