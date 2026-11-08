//! Isolated before/after fixture. Real plans, disk journal and latest-request worker; an
//! in-memory registry, two modeled windows and two 20 ms settings receivers. No desktop API,
//! real config, registry or broadcast is touched. Legacy preserves pre-fix activate behavior.
//! `cargo run -p glaze-tweaks --example measure_style_transitions -- --legacy`

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use glaze_config::look::{Scheme, Style};
use glaze_theme::{Color, WindowsStyle};
use glaze_tweaks::palette::Palette;
use glaze_tweaks::plan::{WindowStyle, system_writes};
use glaze_tweaks::undo::{Store, Undo, Value};
use glaze_tweaks::worker::Worker;
use serde_json::json;

#[derive(Default)]
struct Data {
    values: HashMap<(String, String), Value>,
    writes: usize,
}

#[derive(Clone, Default)]
struct Memory(Arc<Mutex<Data>>);

impl Store for Memory {
    fn read(&self, key: &str, name: &str) -> Result<Option<Value>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .values
            .get(&(key.into(), name.into()))
            .cloned())
    }
    fn write(&mut self, key: &str, name: &str, value: &Value) -> Result<()> {
        let mut data = self.0.lock().unwrap();
        data.writes += 1;
        data.values.insert((key.into(), name.into()), value.clone());
        Ok(())
    }
    fn delete(&mut self, key: &str, name: &str) -> Result<()> {
        let mut data = self.0.lock().unwrap();
        data.writes += 1;
        data.values.remove(&(key.into(), name.into()));
        Ok(())
    }
    fn first_missing(&self, _: &str) -> Result<Option<String>> {
        Ok(None)
    }
    fn delete_if_empty(&mut self, _: &str) -> Result<()> {
        Ok(())
    }
}

#[derive(Default)]
struct Counts {
    broadcasts: usize,
    attributes_per_window: usize,
}

fn broadcast(counts: &Mutex<Counts>) {
    counts.lock().unwrap().broadcasts += 1;
    // A deterministic re-theme workload, rather than a broadcast to the user's apps.
    for _ in 0..2 {
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[derive(Clone)]
struct Input {
    style: WindowsStyle,
    palette: Palette,
}

fn input(dark: bool, accent: u8) -> Input {
    let palette = Palette {
        dominant: Color([4, 5, 6]),
        accent: Color([accent, 90, 180]),
    };
    let look = glaze_config::Look {
        style: Some(Style::Liquid),
        appearance: Some(if dark { Scheme::Dark } else { Scheme::Light }),
        ..Default::default()
    };
    Input {
        style: glaze_theme::windows::resolve(&look, None, Some(palette), false)
            .windows
            .unwrap(),
        palette,
    }
}

struct Session {
    undo: Undo<Memory>,
    current: Input,
    window: WindowStyle,
    counts: Arc<Mutex<Counts>>,
}

impl Session {
    fn apply(&mut self, next: Input, legacy: bool) {
        if self.current.style == next.style && self.current.palette == next.palette {
            return;
        }
        if legacy && self.current.style != next.style {
            // Exact old deactivate -> activate ordering, with the production restore settle.
            self.counts.lock().unwrap().attributes_per_window +=
                self.window.transition_to(WindowStyle::default()).writes();
            self.undo.restore(|| broadcast(&self.counts)).unwrap();
            self.window = WindowStyle::default();
        }
        let writes = system_writes(&next.style, Some(&next.palette));
        let resolved = WindowStyle::resolve(&next.style, Some(&next.palette));
        if legacy {
            if self.undo.apply(&writes).unwrap() {
                broadcast(&self.counts);
            }
            // Old restyle wrote all configured attributes even for a palette-only event.
            self.counts.lock().unwrap().attributes_per_window +=
                WindowStyle::default().transition_to(resolved).writes();
        } else {
            self.undo.prepare(&writes).unwrap();
            self.counts.lock().unwrap().attributes_per_window +=
                self.window.transition_to(resolved).writes();
            self.undo
                .transition(&writes, || broadcast(&self.counts))
                .unwrap();
        }
        self.window = resolved;
        self.current = next;
    }
}

fn run(
    case: &str,
    legacy: bool,
    iteration: usize,
    root: &std::path::Path,
) -> Result<serde_json::Value> {
    let dir = root.join(format!("{case}-{legacy}-{iteration}"));
    std::fs::create_dir_all(&dir)?;
    let memory = Memory::default();
    let counts = Arc::new(Mutex::new(Counts::default()));
    let initial = input(case == "dark-to-light", 30);
    let mut session = Session {
        undo: Undo::new(
            memory.clone(),
            dir.join("undo.json"),
            Duration::from_millis(100),
        ),
        window: WindowStyle::resolve(&initial.style, Some(&initial.palette)),
        current: initial,
        counts: counts.clone(),
    };
    session.undo.apply(&system_writes(
        &session.current.style,
        Some(&session.current.palette),
    ))?;
    memory.0.lock().unwrap().writes = 0;
    let (mut direct, worker) = if legacy {
        (Some(session), None)
    } else {
        let mut session = session;
        (
            None,
            Some(Worker::start(move || {
                Ok(move |next| session.apply(next, false))
            })?),
        )
    };
    let start = Instant::now();
    let mut blocked = Duration::ZERO;
    let events = if case == "palette-drag-20" { 20 } else { 1 };
    for event in 0..events {
        let next = match case {
            "dark-to-light" => input(false, 30),
            "light-to-dark" => input(true, 30),
            _ => input(false, 60 + event),
        };
        // Actual fixture config write+read. Watcher scheduling is excluded in both versions.
        let config = dir.join("config.json");
        std::fs::write(&config, serde_json::to_vec(&next.style)?)?;
        let dispatch = Instant::now();
        let next = Input {
            style: serde_json::from_slice(&std::fs::read(config)?)?,
            ..next
        };
        match (&mut direct, &worker) {
            (Some(session), _) => session.apply(next, true),
            (_, Some(worker)) => worker.submit(next),
            _ => unreachable!(),
        }
        blocked += dispatch.elapsed();
        if events > 1 {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    if let Some(worker) = &worker {
        while !worker.is_idle() {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    let elapsed = start.elapsed();
    let counts = counts.lock().unwrap();
    let result = json!({
        "case":case, "iteration":iteration,
        "config_to_last_fixture_window_settled_ms":elapsed.as_secs_f64()*1000.,
        "main_thread_blocked_ms":blocked.as_secs_f64()*1000.,
        "broadcasts":counts.broadcasts,
        "attribute_writes_per_window":counts.attributes_per_window,
        "registry_mutations":memory.0.lock().unwrap().writes
    });
    drop(counts);
    drop(worker);
    Ok(result)
}

fn main() -> Result<()> {
    let legacy = std::env::args().any(|a| a == "--legacy");
    let root: PathBuf =
        std::env::temp_dir().join(format!("glaze-transition-fixture-{}", std::process::id()));
    std::fs::create_dir_all(&root)?;
    let result = (|| -> Result<_> {
        let mut samples = Vec::new();
        for case in [
            "light-to-dark",
            "dark-to-light",
            "palette-update",
            "palette-drag-20",
        ] {
            for iteration in 0..3 {
                samples.push(run(case, legacy, iteration, &root)?);
            }
        }
        Ok(json!({ "version":if legacy {"legacy"} else {"direct"},
            "fixture":"2 modeled windows; 2 x 20ms receivers; real disk journal and 100ms restore settle; actual worker; excludes file-watcher scheduling, signature IO and real app paint", "samples":samples }))
    })();
    // This is our freshly-created temp directory, never LOCALAPPDATA/glaze.
    std::fs::remove_dir_all(&root)?;
    println!("{}", serde_json::to_string_pretty(&result?)?);
    Ok(())
}
