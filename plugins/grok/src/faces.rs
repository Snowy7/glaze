//! Pure declarative kit faces shared by the component and its renderer fixtures.
use mypaper_plugin_sdk::{Face, Role, SizeClass, Tone, View};
use crate::{config::Routine, discovery::Discovery, routines::{RoutineRun, RoutineState}};

fn row(title: &str, detail: &str, icon: &str, action: Option<String>) -> View {
    View::ListRow { title: title.into(), detail: Some(detail.into()), icon: Some(icon.into()), trailing: None, action }
}
fn button(label: &str, action: &str, icon: &str) -> View {
    View::Button { label: label.into(), action: action.into(), icon: Some(icon.into()), primary: true }
}

pub fn face(id: &str, size: SizeClass, discovery: &Discovery, routines: &[Routine], runs: &[RoutineRun]) -> Face {
    let small = size == SizeClass::Small;
    let body = if !discovery.installed && routines.is_empty() {
        if small { View::stack([View::text("Connect Grok Bot", Role::Headline), button("Connect", "connect", "sparkles")]) }
        else { View::stack([View::empty("sparkles", "Connect Grok Bot", "Detects Grok Bot on this PC. Sign in once in Grok; your account stays there."), button("Connect", "connect", "sparkles")]) }
    } else if id == "prompt" {
        let mut children = vec![View::Field { placeholder: "Ask your Bot…".into(), action: "prompt".into() }];
        if !small { children.extend([View::tinted("Opens a draft in Grok Bot", Role::Caption, Tone::Secondary), View::tinted("for you to review and send.", Role::Caption, Tone::Secondary)]); }
        View::stack(children)
    } else if id == "recent" {
        if small { View::Metric { label: "requests this session".into(), value: runs.len().to_string(), color: None, width: None } }
        else if runs.is_empty() { View::empty("clock", "No recent requests", "Routine receipts appear here. Results open in Grok Bot.") }
        else { View::List { gap: Some(1.0), children: runs.iter().map(|run| {
            let (detail, icon) = match run.state { RoutineState::Sending => ("Sending…", "upload"), RoutineState::Accepted => ("Accepted · open Grok for results", "check"), RoutineState::Failed => ("Request failed · retry routine", "warning") };
            row(&run.title, detail, icon, Some("open".into()))
        }).collect() } }
    } else if routines.is_empty() {
        let mut children = vec![View::text(if discovery.signed_in { "Grok Bot found" } else { "Open Grok Bot" }, Role::Headline)];
        if !small { children.extend([View::tinted("Add a routine webhook under Advanced", Role::Callout, Tone::Secondary), View::tinted("to run it here.", Role::Callout, Tone::Secondary)]); }
        children.push(View::Spacer);
        children.push(button("Open Grok Bot", "open", "external"));
        View::stack(children)
    } else if small {
        let r = &routines[0];
        let busy = runs.iter().any(|run| run.id == r.id && run.state == RoutineState::Sending);
        View::stack([View::text(&r.title, Role::Headline), if busy { View::text("Sending…", Role::Callout) } else { button("Run routine", &format!("run:{}", r.id), "play") }])
    } else {
        View::stack([
            View::Row { gap: Some(4.0), children: vec![View::tinted("Saved routines", Role::Caption, Tone::Secondary), View::IconButton { icon: "more".into(), label: "Advanced".into(), action: "advanced".into(), enabled: true }] },
            View::List { gap: Some(1.0), children: routines.iter().map(|r| {
                let busy = runs.iter().any(|run| run.id == r.id && run.state == RoutineState::Sending);
                row(&r.title, if busy { "Sending request…" } else { "Run now" }, "play", (!busy).then(|| format!("run:{}", r.id)))
            }).collect() },
        ])
    };
    Face { body, refresh: true, footer: Some(if id == "recent" { "Acceptance is a receipt, not completion" } else if discovery.signed_in { "Signed in · detected on this PC" } else if discovery.running { "Sign in once in Grok Bot" } else { "Open Grok Bot to connect" }.into()) }
}

#[cfg(test)]
mod golden;
