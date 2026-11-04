//! "Start with Windows": `mypaper-core --autostart on|off|status`.
//!
//! The preferred mechanism is a per-user Task Scheduler logon task, which Windows starts
//! straight at sign-in instead of waiting for Explorer to process the `Run` key. When the task
//! cannot be registered (Task Scheduler unavailable, access denied) the `Run` value is used.
//! Each command prints one JSON line (see [`State`]) and exits 0, or exits 1 with the error on
//! stderr. Studio's Settings page drives this CLI.

use std::fs::OpenOptions;
use std::os::windows::io::IntoRawHandle;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use serde::Serialize;
use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, HANDLE, RPC_E_CHANGED_MODE,
};
use windows::Win32::Security::Authentication::Identity::{GetUserNameExW, NameSamCompatible};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize,
};
use windows::Win32::System::Console::{
    ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_HANDLE,
    STD_OUTPUT_HANDLE, SetStdHandle,
};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::Win32::System::TaskScheduler::{
    IExecAction, ILogonTrigger, IRegisteredTask, ITaskFolder, ITaskService, TASK_ACTION_EXEC,
    TASK_CREATE_OR_UPDATE, TASK_INSTANCES_IGNORE_NEW, TASK_LOGON_INTERACTIVE_TOKEN,
    TASK_RUNLEVEL_LUA, TASK_TRIGGER_LOGON, TaskScheduler,
};
use windows::Win32::System::Variant::VARIANT;
use windows::core::{BSTR, HRESULT, Interface, PCWSTR, PWSTR};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// Name of the `Run` value, also what earlier versions of Studio wrote.
const RUN_VALUE: &str = "mypaper";
/// `HRESULT_FROM_WIN32` of the "not found" errors Task Scheduler returns for a missing task or folder.
const NOT_FOUND: [HRESULT; 2] = [
    ERROR_FILE_NOT_FOUND.to_hresult(),
    ERROR_PATH_NOT_FOUND.to_hresult(),
];

/// How autostart is currently wired up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Method {
    Task,
    RunKey,
}

/// The line `--autostart` prints.
#[derive(Debug, PartialEq, Eq, Serialize)]
struct State {
    enabled: bool,
    method: Option<Method>,
    /// The quoted exe path Windows will run at sign-in.
    command: Option<String>,
}

/// What Task Scheduler says about our task.
#[derive(Debug, PartialEq, Eq)]
struct TaskInfo {
    enabled: bool,
    /// The action's exe path, unquoted.
    path: String,
}

/// Where the task and the `Run` value live. Tests use their own names; the CLI uses the real ones.
struct Names<'a> {
    task: &'a str,
    run_value: &'a str,
}

/// The program a logon task starts.
struct Action {
    path: PathBuf,
    arguments: String,
}

/// `mypaper (<user>)`: task names are machine-global, so each user gets their own.
fn task_name(sam_user: &str) -> String {
    let user = sam_user.rsplit('\\').next().unwrap_or(sam_user);
    format!("mypaper ({user})")
}

/// Wraps a path in quotes, as the `Run` key and the printed `command` want it.
fn quote(path: &str) -> String {
    format!("\"{path}\"")
}

/// A live, enabled task wins; then the `Run` value; a disabled task alone is reported as disabled.
fn resolve(task: Option<TaskInfo>, run_command: Option<String>) -> State {
    match (task, run_command) {
        (Some(task), _) if task.enabled => State {
            enabled: true,
            method: Some(Method::Task),
            command: Some(quote(&task.path)),
        },
        (_, Some(command)) => State {
            enabled: true,
            method: Some(Method::RunKey),
            command: Some(command),
        },
        (Some(task), None) => State {
            enabled: false,
            method: Some(Method::Task),
            command: Some(quote(&task.path)),
        },
        (None, None) => State {
            enabled: false,
            method: None,
            command: None,
        },
    }
}

/// Runs `--autostart <on|off|status>` when it is the first argument; `None` for a normal start.
pub fn cli(mut args: impl Iterator<Item = String>) -> Option<ExitCode> {
    if args.next().as_deref() != Some("--autostart") {
        return None;
    }
    // A windows-subsystem exe has no console of its own; reach the terminal it was started from.
    ensure_std_handle(STD_OUTPUT_HANDLE);
    ensure_std_handle(STD_ERROR_HANDLE);
    let result = args.next().as_deref().map_or_else(
        || bail!("usage: mypaper-core --autostart on|off|status"),
        run_command,
    );
    Some(
        match result.and_then(|state| Ok(serde_json::to_string(&state)?)) {
            Ok(line) => {
                println!("{line}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e:#}");
                ExitCode::FAILURE
            }
        },
    )
}

fn run_command(command: &str) -> Result<State> {
    let user = current_user()?;
    let task = task_name(&user);
    let names = Names {
        task: &task,
        run_value: RUN_VALUE,
    };
    match command {
        "on" => enable(
            &names,
            &user,
            &std::env::current_exe().context("path of mypaper-core.exe")?,
        ),
        "off" => {
            let state = disable(&names)?;
            // mypaper won't start at sign-in any more, so Windows shouldn't show its still:
            // the user's own wallpaper comes back (docs/performance.md, "Static snapshot").
            if let Err(e) = mypaper_desktop::desktop_wallpaper::restore() {
                eprintln!("could not put your wallpaper back: {e:#}");
            }
            Ok(state)
        }
        "status" => Ok(status(&names)),
        other => bail!("unknown --autostart command {other:?}; use on, off or status"),
    }
}

/// Registers the logon task, or the `Run` value when that fails, and reports the result.
fn enable(names: &Names, user: &str, exe: &Path) -> Result<State> {
    let action = Action {
        path: exe.to_owned(),
        arguments: String::new(),
    };
    match Scheduler::connect().and_then(|scheduler| scheduler.register(names.task, user, &action)) {
        Ok(()) => {
            // Leaving the old Run value would only make Explorer try to start us a second time.
            if let Err(e) = delete_run_value(names.run_value) {
                log::warn!("could not remove the Run value: {e:#}");
            }
        }
        Err(e) => {
            log::warn!("logon task unavailable, using the Run key: {e:#}");
            set_run_value(names.run_value, &quote(&exe.to_string_lossy()))
                .context("writing the Run registry value")?;
        }
    }
    Ok(status(names))
}

/// Removes the task and the `Run` value; whatever is already gone is fine.
fn disable(names: &Names) -> Result<State> {
    let task = Scheduler::connect().and_then(|scheduler| scheduler.delete(names.task));
    let run = delete_run_value(names.run_value);
    task.context("removing the logon task")?;
    run.context("removing the Run registry value")?;
    Ok(status(names))
}

fn status(names: &Names) -> State {
    let task = Scheduler::connect()
        .and_then(|scheduler| scheduler.info(names.task))
        .unwrap_or_else(|e| {
            log::warn!("cannot read the logon task: {e:#}");
            None
        });
    resolve(task, run_value(names.run_value))
}

/// `DOMAIN\user` of the signed-in user.
fn current_user() -> Result<String> {
    let mut buf = [0u16; 256];
    let mut len = buf.len() as u32;
    // SAFETY: `buf` is valid for `len` UTF-16 units; the call writes at most that many and sets `len`.
    let ok = unsafe { GetUserNameExW(NameSamCompatible, Some(PWSTR(buf.as_mut_ptr())), &mut len) };
    if !ok {
        bail!(
            "GetUserNameExW failed: {}",
            windows::core::Error::from_thread()
        );
    }
    Ok(String::from_utf16_lossy(&buf[..len as usize]))
}

/// If a std handle is missing (GUI-subsystem exe started from a terminal), attaches to the
/// parent's console and points the handle at it.
fn ensure_std_handle(which: STD_HANDLE) {
    // SAFETY: plain handle queries; the opened console handle is leaked on purpose so it stays
    // valid for the life of the process.
    unsafe {
        if GetStdHandle(which).is_ok_and(|h| !h.is_invalid() && !h.0.is_null()) {
            return;
        }
        // Fails harmlessly when already attached or when there is no parent console.
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        if let Ok(conout) = OpenOptions::new().write(true).open("CONOUT$") {
            let _ = SetStdHandle(which, HANDLE(conout.into_raw_handle()));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Task Scheduler
// ---------------------------------------------------------------------------------------------

/// A connection to the local Task Scheduler, rooted at `\`.
struct Scheduler {
    // Declaration order is drop order: COM interfaces go before `CoUninitialize`.
    root: ITaskFolder,
    service: ITaskService,
    _com: ComGuard,
}

/// Balances a successful `CoInitializeEx`; a thread already in another apartment is left alone.
struct ComGuard(bool);

impl ComGuard {
    fn new() -> Result<Self> {
        // SAFETY: per-thread COM initialisation, balanced in `Drop` when it succeeded.
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if hr == RPC_E_CHANGED_MODE {
            return Ok(Self(false));
        }
        hr.ok().context("initializing COM")?;
        Ok(Self(true))
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: balances the successful `CoInitializeEx` in `new`.
            unsafe { CoUninitialize() };
        }
    }
}

/// `true` for the errors Task Scheduler returns when a task or folder does not exist.
fn is_not_found(e: &windows::core::Error) -> bool {
    NOT_FOUND.contains(&e.code())
}

impl Scheduler {
    fn connect() -> Result<Self> {
        let com = ComGuard::new()?;
        // SAFETY: COM is initialised on this thread; the empty VARIANTs mean "local machine, current user".
        let (service, root) = unsafe {
            let service: ITaskService =
                CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)
                    .context("creating the Task Scheduler service")?;
            let none = VARIANT::default();
            service
                .Connect(&none, &none, &none, &none)
                .context("connecting to Task Scheduler")?;
            let root = service
                .GetFolder(&BSTR::from("\\"))
                .context("opening the root task folder")?;
            (service, root)
        };
        Ok(Self {
            root,
            service,
            _com: com,
        })
    }

    /// Creates or replaces the per-user logon task `task` (a path under `\`) running `action`.
    ///
    /// The task is limited (no UAC elevation) and runs in the user's interactive session. Defaults
    /// that would hurt a long-running desktop shell are overridden: the 72 h execution limit, the
    /// battery restrictions, and the below-normal priority (7) that also lowers I/O priority.
    fn register(&self, task: &str, user: &str, action: &Action) -> Result<()> {
        let user_bstr = BSTR::from(user);
        // SAFETY: COM calls on interface pointers owned by this function; every out-parameter is a
        // live local and every BSTR/VARIANT outlives its call.
        unsafe {
            let def = self.service.NewTask(0)?;
            def.RegistrationInfo()?
                .SetDescription(&BSTR::from("Starts mypaper when you sign in."))?;

            let trigger = def
                .Triggers()?
                .Create(TASK_TRIGGER_LOGON)?
                .cast::<ILogonTrigger>()?;
            trigger.SetUserId(&user_bstr)?;

            let principal = def.Principal()?;
            principal.SetUserId(&user_bstr)?;
            principal.SetLogonType(TASK_LOGON_INTERACTIVE_TOKEN)?;
            principal.SetRunLevel(TASK_RUNLEVEL_LUA)?;

            let settings = def.Settings()?;
            settings.SetDisallowStartIfOnBatteries(false.into())?;
            settings.SetStopIfGoingOnBatteries(false.into())?;
            settings.SetExecutionTimeLimit(&BSTR::from("PT0S"))?;
            settings.SetPriority(4)?;
            settings.SetMultipleInstances(TASK_INSTANCES_IGNORE_NEW)?;
            settings.SetAllowDemandStart(true.into())?;
            settings.SetHidden(true.into())?;
            settings.SetEnabled(true.into())?;

            let exec = def
                .Actions()?
                .Create(TASK_ACTION_EXEC)?
                .cast::<IExecAction>()?;
            exec.SetPath(&BSTR::from(action.path.to_string_lossy().as_ref()))?;
            if !action.arguments.is_empty() {
                exec.SetArguments(&BSTR::from(action.arguments.as_str()))?;
            }
            if let Some(dir) = action.path.parent() {
                exec.SetWorkingDirectory(&BSTR::from(dir.to_string_lossy().as_ref()))?;
            }

            self.root
                .RegisterTaskDefinition(
                    &BSTR::from(task),
                    &def,
                    TASK_CREATE_OR_UPDATE.0,
                    &VARIANT::from(user),
                    &VARIANT::default(),
                    TASK_LOGON_INTERACTIVE_TOKEN,
                    &VARIANT::default(),
                )
                .with_context(|| format!("registering task {task:?}"))?;
        }
        Ok(())
    }

    /// The registered task, or `None` when there is none.
    fn get(&self, task: &str) -> Result<Option<IRegisteredTask>> {
        // SAFETY: COM call on an owned interface with a live BSTR.
        match unsafe { self.root.GetTask(&BSTR::from(task)) } {
            Ok(task) => Ok(Some(task)),
            Err(e) if is_not_found(&e) => Ok(None),
            Err(e) => Err(e).with_context(|| format!("reading task {task:?}")),
        }
    }

    fn info(&self, task: &str) -> Result<Option<TaskInfo>> {
        let Some(task) = self.get(task)? else {
            return Ok(None);
        };
        // SAFETY: COM calls on interfaces owned here; `path` is a live out-parameter.
        unsafe {
            let enabled = task.Enabled()?.as_bool();
            let exec = task
                .Definition()?
                .Actions()?
                .get_Item(1)?
                .cast::<IExecAction>()?;
            let mut path = BSTR::new();
            exec.Path(&mut path)?;
            Ok(Some(TaskInfo {
                enabled,
                path: path.to_string(),
            }))
        }
    }

    /// Deletes the task; a missing task is fine.
    fn delete(&self, task: &str) -> Result<()> {
        // SAFETY: COM call on an owned interface with a live BSTR.
        match unsafe { self.root.DeleteTask(&BSTR::from(task), 0) } {
            Err(e) if !is_not_found(&e) => {
                Err(e).with_context(|| format!("deleting task {task:?}"))
            }
            _ => Ok(()),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Run key
// ---------------------------------------------------------------------------------------------

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// The `Run` value's command, if present.
fn run_value(name: &str) -> Option<String> {
    let (key, name) = (wide(RUN_KEY), wide(name));
    let mut bytes = 0u32;
    // SAFETY: both calls get NUL-terminated names; the first only asks for the size, the second
    // gets a buffer of exactly that many bytes.
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut bytes),
        )
        .ok()
        .ok()?;
        let mut buf = vec![0u16; (bytes as usize).div_ceil(2)];
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
        .ok()
        .ok()?;
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    }
}

fn set_run_value(name: &str, command: &str) -> Result<()> {
    let (key, name, data) = (wide(RUN_KEY), wide(name), wide(command));
    // SAFETY: NUL-terminated names; `data` is valid for the byte count passed alongside it.
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            REG_SZ.0,
            Some(data.as_ptr().cast()),
            (data.len() * 2) as u32,
        )
        .ok()?;
    }
    Ok(())
}

/// Deletes the `Run` value; a missing value is fine.
fn delete_run_value(name: &str) -> Result<()> {
    let (key, name) = (wide(RUN_KEY), wide(name));
    // SAFETY: NUL-terminated names.
    let result = unsafe {
        RegDeleteKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
        )
    };
    if result == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        Ok(result.ok()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(enabled: bool) -> Option<TaskInfo> {
        Some(TaskInfo {
            enabled,
            path: r"C:\mypaper\mypaper-core.exe".into(),
        })
    }

    #[test]
    fn task_name_drops_the_domain() {
        assert_eq!(task_name(r"DESKTOP-1\you"), "mypaper (you)");
        assert_eq!(task_name("you"), "mypaper (you)");
    }

    #[test]
    fn enabled_task_wins_over_run_value() {
        let state = resolve(task(true), Some("\"old.exe\"".into()));
        assert_eq!(state.method, Some(Method::Task));
        assert_eq!(
            state.command.as_deref(),
            Some(r#""C:\mypaper\mypaper-core.exe""#)
        );
    }

    #[test]
    fn run_value_covers_a_missing_or_disabled_task() {
        for task in [None, task(false)] {
            let state = resolve(task, Some("\"run.exe\"".into()));
            assert_eq!(
                state,
                State {
                    enabled: true,
                    method: Some(Method::RunKey),
                    command: Some("\"run.exe\"".into())
                }
            );
        }
    }

    #[test]
    fn disabled_task_alone_is_off_but_still_reported() {
        let state = resolve(task(false), None);
        assert!(!state.enabled);
        assert_eq!(state.method, Some(Method::Task));
        assert_eq!(
            resolve(None, None),
            State {
                enabled: false,
                method: None,
                command: None
            }
        );
    }

    #[test]
    fn state_serializes_to_the_documented_line() {
        let run_key = State {
            enabled: true,
            method: Some(Method::RunKey),
            command: Some(r#""C:\a b\core.exe""#.into()),
        };
        assert_eq!(
            serde_json::to_string(&run_key).unwrap(),
            r#"{"enabled":true,"method":"run-key","command":"\"C:\\a b\\core.exe\""}"#
        );
        let off = State {
            enabled: false,
            method: None,
            command: None,
        };
        assert_eq!(
            serde_json::to_string(&off).unwrap(),
            r#"{"enabled":false,"method":null,"command":null}"#
        );
    }

    /// Deletes the test task and `Run` value when dropped, also when the test fails.
    struct Cleanup {
        task: String,
        run_value: String,
    }

    impl Cleanup {
        fn new(label: &str) -> Self {
            let id = format!("mypaper-autostart-test-{label}-{}", std::process::id());
            Self {
                task: id.clone(),
                run_value: id,
            }
        }
    }

    impl Drop for Cleanup {
        fn drop(&mut self) {
            if let Ok(scheduler) = Scheduler::connect() {
                let _ = scheduler.delete(&self.task);
            }
            let _ = delete_run_value(&self.run_value);
        }
    }

    #[test]
    fn run_value_round_trip() {
        let cleanup = Cleanup::new("runkey");
        assert_eq!(run_value(&cleanup.run_value), None);
        set_run_value(
            &cleanup.run_value,
            r#""C:\Program Files\mypaper\mypaper-core.exe""#,
        )
        .unwrap();
        assert_eq!(
            run_value(&cleanup.run_value).as_deref(),
            Some(r#""C:\Program Files\mypaper\mypaper-core.exe""#)
        );
        delete_run_value(&cleanup.run_value).unwrap();
        assert_eq!(run_value(&cleanup.run_value), None);
        delete_run_value(&cleanup.run_value).unwrap(); // already gone
    }

    /// Registers a task exactly as `on` does (harmless action), reads it back, runs it on demand
    /// and checks it succeeded. `prefix` puts it in a subfolder of `\` (e.g. `"mypaper-x\\"`).
    fn logon_task_round_trip(prefix: &str) {
        use std::time::{Duration, Instant};
        use windows::Win32::System::TaskScheduler::{TASK_RUNLEVEL_TYPE, TASK_STATE_RUNNING};

        let cleanup = Cleanup::new("task");
        let path = format!("{prefix}{}", cleanup.task);
        let user = current_user().unwrap();
        let scheduler = Scheduler::connect().unwrap();
        let cmd = PathBuf::from(r"C:\Windows\System32\cmd.exe");
        let action = Action {
            path: cmd.clone(),
            arguments: "/c exit 0".into(),
        };

        let result = scheduler.register(&path, &user, &action);
        // Report the exact HRESULT: this test exists to find out whether a limited user may do this.
        if let Err(e) = &result {
            let code = e
                .chain()
                .find_map(|c| c.downcast_ref::<windows::core::Error>())
                .map(|e| e.code());
            panic!("register {path:?} failed: {e:#} (hresult {code:?})");
        }
        let task = scheduler
            .get(&path)
            .unwrap()
            .expect("registered task is found");

        // SAFETY: COM calls on interfaces owned by this test; out-parameters are live locals.
        unsafe {
            let def = task.Definition().unwrap();
            let mut trigger_user = BSTR::new();
            def.Triggers()
                .unwrap()
                .get_Item(1)
                .unwrap()
                .cast::<ILogonTrigger>()
                .unwrap()
                .UserId(&mut trigger_user)
                .unwrap();
            assert_eq!(trigger_user.to_string(), user);

            let principal = def.Principal().unwrap();
            let mut run_level = TASK_RUNLEVEL_TYPE::default();
            principal.RunLevel(&mut run_level).unwrap();
            assert_eq!(run_level, TASK_RUNLEVEL_LUA, "must stay limited");

            let settings = def.Settings().unwrap();
            let (mut limit, mut priority, mut battery, mut stop, mut hidden) =
                (BSTR::new(), 0, true.into(), true.into(), false.into());
            settings.ExecutionTimeLimit(&mut limit).unwrap();
            settings.Priority(&mut priority).unwrap();
            settings.DisallowStartIfOnBatteries(&mut battery).unwrap();
            settings.StopIfGoingOnBatteries(&mut stop).unwrap();
            settings.Hidden(&mut hidden).unwrap();
            assert_eq!(limit.to_string(), "PT0S");
            assert_eq!(priority, 4);
            assert!(!battery.as_bool() && !stop.as_bool() && hidden.as_bool());

            // On demand, then wait for the (instant) action to finish.
            task.Run(&VARIANT::default())
                .expect("running the task on demand");
            let deadline = Instant::now() + Duration::from_secs(15);
            const NOT_YET_RUN: i32 = 0x0004_1303; // SCHED_S_TASK_HAS_NOT_RUN
            loop {
                let last = task.LastTaskResult().unwrap();
                if task.State().unwrap() != TASK_STATE_RUNNING && last != NOT_YET_RUN {
                    assert_eq!(last, 0, "LastTaskResult");
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "task did not finish in time (last result {last:#x})"
                );
                std::thread::sleep(Duration::from_millis(100));
            }
        }

        let info = scheduler.info(&path).unwrap().expect("info");
        assert_eq!(
            info,
            TaskInfo {
                enabled: true,
                path: cmd.to_string_lossy().into_owned()
            }
        );
        scheduler.delete(&path).unwrap();
        assert!(scheduler.get(&path).unwrap().is_none());
    }

    /// Can a non-elevated user register this task in the root folder? Run explicitly:
    /// `cargo test -p mypaper-core -- --ignored --nocapture`. Touches the real Task Scheduler
    /// (test-named task, always removed).
    #[test]
    #[ignore = "touches Task Scheduler"]
    fn limited_logon_task_in_root_folder() {
        logon_task_round_trip("");
    }

    /// The same inside a `\mypaper-autostart-test-...\` subfolder (removed afterwards).
    #[test]
    #[ignore = "touches Task Scheduler"]
    fn limited_logon_task_in_subfolder() {
        let folder = format!("mypaper-autostart-test-folder-{}", std::process::id());
        struct FolderGuard(String);
        impl Drop for FolderGuard {
            fn drop(&mut self) {
                if let Ok(scheduler) = Scheduler::connect() {
                    // SAFETY: COM call on an owned interface with a live BSTR.
                    let _ = unsafe { scheduler.root.DeleteFolder(&BSTR::from(self.0.as_str()), 0) };
                }
            }
        }
        let _guard = FolderGuard(folder.clone());
        logon_task_round_trip(&format!("{folder}\\"));
    }

    /// Both ends of `enable`/`disable` against test names, including the Run-key bookkeeping.
    #[test]
    #[ignore = "touches Task Scheduler"]
    fn enable_then_disable_with_test_names() {
        let cleanup = Cleanup::new("flow");
        let names = Names {
            task: &cleanup.task,
            run_value: &cleanup.run_value,
        };
        let user = current_user().unwrap();
        let exe = std::env::current_exe().unwrap();
        set_run_value(names.run_value, "\"stale.exe\"").unwrap();

        let on = enable(&names, &user, &exe).unwrap();
        assert_eq!(
            on.method,
            Some(Method::Task),
            "task registered, state: {on:?}"
        );
        assert_eq!(on.command, Some(quote(&exe.to_string_lossy())));
        assert_eq!(
            run_value(names.run_value),
            None,
            "a registered task replaces the Run value"
        );

        let off = disable(&names).unwrap();
        assert_eq!(
            off,
            State {
                enabled: false,
                method: None,
                command: None
            }
        );
        disable(&names).unwrap(); // idempotent
    }
}
