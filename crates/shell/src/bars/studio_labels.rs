//! Desktop name chips describe the module's actual left-click action.
use mypaper_bars::{
    BarsConfig, Module, ModuleRef,
    config::{Action, BuiltinAction},
};

fn title(value: &str) -> String {
    let value = value.replace(['-', '_'], " ");
    let mut chars = value.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars.take(63)).collect()
    })
}

fn target(value: &str) -> String {
    std::path::Path::new(value)
        .file_name()
        .map_or(value, |s| s.to_str().unwrap_or(value))
        .chars()
        .take(64)
        .collect()
}

pub(super) fn label(module: &ModuleRef, config: &BarsConfig, fluent: bool) -> String {
    let id = serde_json::to_value(module)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    let name = match module {
        ModuleRef::Custom(id) => title(id.split("--").next().unwrap_or(id)),
        _ => title(&id),
    };
    let own = config
        .modules
        .get(module)
        .and_then(|o| o.click.left.as_ref());
    let action = match own {
        Some(Action::Exec { exec, .. }) => format!("Run {}", target(exec)),
        Some(Action::Open { open }) => format!("Open {}", target(open)),
        Some(Action::Builtin(action)) if *action != BuiltinAction::Default => {
            serde_json::to_value(action)
                .ok()
                .and_then(|v| v.as_str().map(title))
                .unwrap_or_default()
        }
        _ => match module {
            ModuleRef::Builtin(module) => match module {
                Module::Start => "Launcher",
                Module::Search => "Search",
                Module::Clock | Module::Notifications => "Notifications",
                Module::Quick => "Quick settings",
                Module::Network if fluent => "Quick settings",
                Module::Network => "Wi-Fi",
                Module::Volume if fluent => "Quick settings",
                Module::Volume => "Volume mixer",
                Module::Battery if fluent => "Quick settings",
                Module::Battery => "Battery settings",
                Module::Power => "Power menu",
                Module::Language => "Keyboard language",
                Module::Stats => "Task Manager",
                Module::Media => "Play or pause",
                Module::Workspaces => "Switch workspace",
                Module::Tray => "Tray icon's action",
                Module::App => "No action",
            }
            .into(),
            ModuleRef::Custom(id) => super::plugin_modules::action_name(id)
                .map(|a| title(&a))
                .or_else(|| {
                    super::plugin_modules::panel_of(id)
                        .map(|p| format!("Open {}", title(p.rsplit("--").next().unwrap_or(&p))))
                })
                .unwrap_or_else(|| "Plugin action".into()),
            ModuleRef::Break => "No action".into(),
        },
    };
    format!("{name} · Bar click: {action}")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chips_name_configured_actions_and_the_actual_family_default() {
        let config = BarsConfig::parse(r#"{"modules":{"clock":{"click":{"left":"calendar"}},"volume":{"click":{"left":{"exec":"C:\\Tools\\mixer.exe"}}}}}"#).unwrap();
        assert_eq!(
            label(&ModuleRef::Builtin(Module::Clock), &config, false),
            "Clock · Bar click: Calendar"
        );
        assert_eq!(
            label(&ModuleRef::Builtin(Module::Volume), &config, false),
            "Volume · Bar click: Run mixer.exe"
        );
        assert_eq!(
            label(&ModuleRef::Builtin(Module::Network), &config, true),
            "Network · Bar click: Quick settings"
        );
    }
}
