//! The engine's existing glass wire, shared with the shell. App glass is a normal top-level
//! window below our HWND; menu glass uses the popup layer. There is no GPU device here.
#[allow(
    dead_code,
    reason = "the receiving half of the shared wire is engine-owned"
)]
#[path = "../../wallpaper/src/overlay/request.rs"]
mod wire;

use glaze_ui::glass::{Backdrop, GlassMaterial, Shape};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::{Accessibility::*, WindowsAndMessaging::*},
};
use wire::{Channel, Layer, Material, Reply, Request};

pub const CHANGED: u32 = WM_APP + 173;
thread_local! { static OWNER: Cell<HWND> = const { Cell::new(HWND(std::ptr::null_mut())) }; }
pub struct Client {
    engine: Cell<Option<HWND>>,
    plates: RefCell<Vec<Plate>>,
    hook: Cell<HWINEVENTHOOK>,
}
struct Plate {
    client: HWND,
    shape: Shape,
    radius: f32,
    material: Material,
    channel: Channel,
    overlay: Cell<Option<HWND>>,
}
impl Plate {
    fn request(&self) -> Request {
        let mut class = [0u16; 64];
        // SAFETY: the bounded class buffer is writable and the client HWND is only queried.
        let count = unsafe { GetClassNameW(self.client, &mut class) } as usize;
        let app = String::from_utf16_lossy(&class[..count]) == "glaze.ui.app";
        Request::Open {
            client: self.client,
            layer: if app { Layer::Desktop } else { Layer::Popup },
            rect: glaze_desktop::Rect {
                x: self.shape.x,
                y: self.shape.y,
                width: self.shape.width,
                height: self.shape.height,
            },
            radius: self.radius,
            material: self.material,
        }
    }
}
impl Client {
    pub fn install(owner: HWND) -> Rc<Self> {
        OWNER.set(owner);
        let client = Rc::new(Self {
            engine: Cell::new(None),
            plates: RefCell::new(Vec::new()),
            hook: Cell::new(HWINEVENTHOOK::default()),
        });
        glaze_ui::glass::install(client.clone());
        client
    }
    fn engine(&self) -> Option<HWND> {
        if let Some(engine) = self
            .engine
            .get()
            // SAFETY: IsWindow accepts a possibly stale handle and does not dereference application memory.
            .filter(|h| unsafe { IsWindow(Some(*h)) }.as_bool())
        {
            return Some(engine);
        }
        let engine = wire::find_engine();
        if engine != self.engine.replace(engine) {
            self.plates.borrow_mut().clear();
            let old = self.hook.replace(HWINEVENTHOOK::default());
            if !old.is_invalid() {
                // SAFETY: this hook belongs to this client and is removed exactly once here.
                let _ = unsafe { UnhookWinEvent(old) };
            }
            if let Some(engine) = engine {
                let mut pid = 0;
                // SAFETY: the live engine HWND is queried into a writable process-ID local.
                unsafe {
                    GetWindowThreadProcessId(engine, Some(&mut pid));
                }
                // SAFETY: the static out-of-context callback runs on this pumping UI thread, scoped to the engine PID.
                self.hook.set(unsafe {
                    SetWinEventHook(
                        EVENT_OBJECT_DESTROY,
                        EVENT_OBJECT_SHOW,
                        None,
                        Some(changed),
                        pid,
                        0,
                        WINEVENT_OUTOFCONTEXT,
                    )
                });
            }
        }
        engine
    }
    fn send(&self, request: &Request) -> bool {
        let Some(engine) = self.engine() else {
            return false;
        };
        match wire::send(engine, request) {
            Reply::Taken => true,
            Reply::TakenExcludeYourself => {
                let _ =
                    // SAFETY: the request names one of our own HWNDs; this changes only its capture affinity.
                    unsafe { SetWindowDisplayAffinity(request.client(), WDA_EXCLUDEFROMCAPTURE) };
                true
            }
            Reply::Rejected => {
                self.engine.set(None);
                false
            }
        }
    }
    pub fn contains(&self, hwnd: HWND) -> bool {
        self.plates.borrow().iter().any(|p| p.client == hwnd)
    }

    /// At creation the engine puts its glass directly under the requesting HWND. Remember that
    /// exact adjacent window, checking its class and process; never guess among other plates.
    pub fn remember_overlay(&self, client: HWND) {
        let Some(engine) = self.engine() else {
            return;
        };
        let plates = self.plates.borrow();
        let Some(plate) = plates.iter().find(|p| p.client == client) else {
            return;
        };
        if plate
            .overlay
            .get()
            // SAFETY: IsWindow accepts a stale overlay HWND and only queries its existence.
            .is_some_and(|h| unsafe { IsWindow(Some(h)) }.as_bool())
        {
            return;
        }
        // SAFETY: this is a read-only z-order query for the live client window.
        if let Ok(next) = unsafe { GetWindow(client, GW_HWNDNEXT) } {
            let (mut engine_pid, mut overlay_pid) = (0, 0);
            let mut class = [0u16; 64];
            // SAFETY: both handles are queried into writable process-ID locals.
            unsafe {
                GetWindowThreadProcessId(engine, Some(&mut engine_pid));
                GetWindowThreadProcessId(next, Some(&mut overlay_pid));
            }
            // SAFETY: the bounded class buffer is writable for the class-name query.
            let length = unsafe { GetClassNameW(next, &mut class) }.max(0) as usize;
            if engine_pid == overlay_pid
                && String::from_utf16_lossy(&class[..length]) == "glaze.wallpaper.glass"
            {
                plate.overlay.set(Some(next));
            }
        }
    }

    /// A foreground or visibility transition also needs to put the glass under its client.
    pub fn restack(&self, hwnd: HWND) {
        self.remember_overlay(hwnd);
        if let Some(plate) = self.plates.borrow().iter().find(|p| p.client == hwnd)
            && let Some(overlay) = plate.overlay.get()
        {
            // Only the verified engine window for our plate moves. A normal app can rise
            // above another app without moving; keep its glass beside it in the z-order.
            // SAFETY: only the verified engine overlay for our client is reordered; geometry and activation stay unchanged.
            let _ = unsafe {
                SetWindowPos(
                    overlay,
                    Some(hwnd),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                )
            };
        }
    }
}
impl Backdrop for Client {
    fn available(&self) -> bool {
        self.engine().is_some()
    }
    fn open(&self, client: HWND, shape: Shape, radius: f32, material: &GlassMaterial) -> bool {
        let Ok(channel) = Channel::open(client, true) else {
            return false;
        };
        let mut plate = Plate {
            client,
            shape,
            radius,
            material: material_wire(material),
            channel,
            overlay: Cell::new(None),
        };
        let request = plate.request();
        plate.channel.write(&request);
        if !self.send(&request) {
            return false;
        }
        let mut plates = self.plates.borrow_mut();
        plates.retain(|p| p.client != client);
        plates.push(plate);
        true
    }
    fn place(&self, client: HWND, shape: Shape, radius: f32) {
        let Some(engine) = self.engine() else {
            return;
        };
        let mut plates = self.plates.borrow_mut();
        let Some(plate) = plates.iter_mut().find(|p| p.client == client) else {
            return;
        };
        if plate.shape == shape && plate.radius == radius {
            return;
        }
        plate.shape = shape;
        plate.radius = radius;
        let request = plate.request();
        let _ = plate.channel.publish(engine, &request);
    }
    fn material(&self, client: HWND, material: &GlassMaterial) {
        let Some(engine) = self.engine() else {
            return;
        };
        let mut plates = self.plates.borrow_mut();
        let Some(plate) = plates.iter_mut().find(|p| p.client == client) else {
            return;
        };
        let material = material_wire(material);
        if plate.material == material {
            return;
        }
        plate.material = material;
        let request = plate.request();
        let _ = plate.channel.publish(engine, &request);
    }
    fn close(&self, client: HWND) {
        if self.contains(client) {
            self.send(&Request::Close { client });
        }
        self.plates.borrow_mut().retain(|p| p.client != client);
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        if let Some(engine) = self.engine() {
            for plate in self.plates.borrow().iter() {
                wire::send(
                    engine,
                    &Request::Close {
                        client: plate.client,
                    },
                );
            }
        }
        let hook = self.hook.get();
        if !hook.is_invalid() {
            // SAFETY: this client owns the registered event hook and closes it once.
            let _ = unsafe { UnhookWinEvent(hook) };
        }
    }
}
unsafe extern "system" fn changed(
    _: HWINEVENTHOOK,
    _: u32,
    _: HWND,
    object: i32,
    child: i32,
    _: u32,
    _: u32,
) {
    if object == 0 && child == 0 {
        let owner = OWNER.get();
        // SAFETY: this is a pointer-free wake; an already closed owner HWND simply rejects it.
        let _ = unsafe { PostMessageW(Some(owner), CHANGED, WPARAM(0), LPARAM(0)) };
    }
}
fn material_wire(material: &GlassMaterial) -> Material {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Material {
        clarity: material.clarity,
        blur: material.blur,
        refraction: material.refraction,
        highlight: material.highlight,
        tint: [
            byte(material.tint.r),
            byte(material.tint.g),
            byte(material.tint.b),
            0,
        ],
    }
}
