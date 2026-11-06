//! The engine's existing glass wire, shared with the shell. App glass is a normal top-level
//! window below our HWND; menu glass uses the popup layer. There is no GPU device here.
#[allow(dead_code, reason = "the receiving half of the shared wire is engine-owned")]
#[path = "../../wallpaper/src/overlay/request.rs"]
mod wire;

use std::{cell::{Cell, RefCell}, rc::Rc};
use mypaper_ui::glass::{Backdrop, GlassMaterial, Shape};
use windows::Win32::{Foundation::HWND, UI::WindowsAndMessaging::*};
use wire::{Channel, Layer, Material, Reply, Request};

pub struct Client { engine: Cell<Option<HWND>>, plates: RefCell<Vec<Plate>> }
struct Plate { client: HWND, shape: Shape, radius: f32, material: Material, channel: Channel }
impl Plate {
    fn request(&self) -> Request {
        let mut class = [0u16; 64];
        let count = unsafe { GetClassNameW(self.client, &mut class) } as usize;
        let app = String::from_utf16_lossy(&class[..count]) == "mypaper.ui.app";
        Request::Open { client: self.client, layer: if app { Layer::Desktop } else { Layer::Popup },
            rect: mypaper_desktop::Rect { x: self.shape.x, y: self.shape.y, width: self.shape.width, height: self.shape.height },
            radius: self.radius, material: self.material }
    }
}
impl Client {
    pub fn install() -> Rc<Self> {
        let client = Rc::new(Self { engine: Cell::new(None), plates: RefCell::new(Vec::new()) });
        mypaper_ui::glass::install(client.clone());
        client
    }
    fn engine(&self) -> Option<HWND> {
        if let Some(engine) = self.engine.get().filter(|h| unsafe { IsWindow(Some(*h)) }.as_bool()) { return Some(engine); }
        let engine = wire::find_engine(); self.engine.set(engine); engine
    }
    fn send(&self, request: &Request) -> bool {
        let Some(engine) = self.engine() else { return false; };
        match wire::send(engine, request) {
            Reply::Taken => true,
            Reply::TakenExcludeYourself => { let _ = unsafe { SetWindowDisplayAffinity(request.client(), WDA_EXCLUDEFROMCAPTURE) }; true }
            Reply::Rejected => { self.engine.set(None); false }
        }
    }
    pub fn contains(&self, hwnd: HWND) -> bool { self.plates.borrow().iter().any(|p| p.client == hwnd) }

    /// A foreground or visibility transition also needs to put the glass under its client.
    pub fn restack(&self, hwnd: HWND) {
        if let Some(plate) = self.plates.borrow().iter().find(|p| p.client == hwnd) {
            self.send(&Request::Place { client: hwnd,
                rect: mypaper_desktop::Rect { x: plate.shape.x, y: plate.shape.y, width: plate.shape.width, height: plate.shape.height }, radius: plate.radius });
        }
    }
}
impl Backdrop for Client {
    fn available(&self) -> bool { self.engine().is_some() }
    fn open(&self, client: HWND, shape: Shape, radius: f32, material: &GlassMaterial) -> bool {
        let Ok(channel) = Channel::open(client, true) else { return false; };
        let mut plate = Plate { client, shape, radius, material: material_wire(material), channel };
        let request = plate.request();
        plate.channel.write(&request);
        if !self.send(&request) { return false; }
        let mut plates = self.plates.borrow_mut();
        plates.retain(|p| p.client != client); plates.push(plate); true
    }
    fn place(&self, client: HWND, shape: Shape, radius: f32) {
        let Some(engine) = self.engine() else { return; };
        let mut plates = self.plates.borrow_mut();
        let Some(plate) = plates.iter_mut().find(|p| p.client == client) else { return; };
        if plate.shape == shape && plate.radius == radius { return; }
        plate.shape = shape; plate.radius = radius;
        let request = plate.request(); let _ = plate.channel.publish(engine, &request);
    }
    fn material(&self, client: HWND, material: &GlassMaterial) {
        let Some(engine) = self.engine() else { return; };
        let mut plates = self.plates.borrow_mut();
        let Some(plate) = plates.iter_mut().find(|p| p.client == client) else { return; };
        let material = material_wire(material);
        if plate.material == material { return; }
        plate.material = material;
        let request = plate.request(); let _ = plate.channel.publish(engine, &request);
    }
    fn close(&self, client: HWND) {
        if self.contains(client) { self.send(&Request::Close { client }); }
        self.plates.borrow_mut().retain(|p| p.client != client);
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        if let Some(engine) = self.engine() {
            for plate in self.plates.borrow().iter() { wire::send(engine, &Request::Close { client: plate.client }); }
        }
    }
}
fn material_wire(material: &GlassMaterial) -> Material {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Material { clarity: material.clarity, blur: material.blur, refraction: material.refraction, highlight: material.highlight,
        tint: [byte(material.tint.r), byte(material.tint.g), byte(material.tint.b), 0] }
}
