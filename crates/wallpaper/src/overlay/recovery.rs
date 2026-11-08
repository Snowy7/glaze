//! CPU-only overlay requests survive a capture/device rebuild, including changes during retry.

use super::request::{Layer, Material, Request, Style};
use glaze_desktop::Rect;
use windows::Win32::Foundation::HWND;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Desired {
    pub client: HWND,
    pub layer: Layer,
    pub rect: Rect,
    pub radius: f32,
    pub material: Material,
    pub style: Style,
}

impl Desired {
    pub fn open(self) -> Request {
        Request::Open {
            client: self.client,
            layer: self.layer,
            rect: self.rect,
            radius: self.radius,
            material: self.material,
        }
    }
}

#[derive(Default)]
pub(super) struct Requests(Vec<Desired>);

impl Requests {
    pub fn remember(&mut self, request: Request) {
        let client = request.client();
        if let Request::Close { .. } = request {
            self.0.retain(|o| o.client != client);
            return;
        }
        let existing = self.0.iter_mut().find(|o| o.client == client);
        if let Request::Open {
            layer,
            rect,
            radius,
            material,
            ..
        } = request
        {
            if let Some(o) = existing {
                (o.layer, o.rect, o.radius, o.material) = (layer, rect, radius, material);
            } else {
                self.0.push(Desired {
                    client,
                    layer,
                    rect,
                    radius,
                    material,
                    style: Style::DEFAULT,
                });
            }
        } else if let Some(o) = existing {
            match request {
                Request::Place { rect, radius, .. } => (o.rect, o.radius) = (rect, radius),
                Request::Material { material, .. } => o.material = material,
                Request::Style { style, .. } => o.style = style,
                _ => {}
            }
        }
    }

    pub fn live(&mut self, alive: impl Fn(HWND) -> bool) -> Vec<Desired> {
        self.0.retain(|o| alive(o.client));
        self.0.clone()
    }
}

pub(super) fn device_lost(error: &anyhow::Error) -> bool {
    use windows::Win32::Graphics::Dxgi::{
        DXGI_ERROR_DEVICE_HUNG, DXGI_ERROR_DEVICE_REMOVED, DXGI_ERROR_DEVICE_RESET,
        DXGI_ERROR_DRIVER_INTERNAL_ERROR,
    };
    error
        .chain()
        .filter_map(|e| e.downcast_ref::<windows::core::Error>())
        .any(|e| {
            matches!(
                e.code(),
                DXGI_ERROR_DEVICE_REMOVED
                    | DXGI_ERROR_DEVICE_HUNG
                    | DXGI_ERROR_DEVICE_RESET
                    | DXGI_ERROR_DRIVER_INTERNAL_ERROR
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lost_device_reopens_latest_requests_without_resurrecting_closed_clients() {
        let a = HWND(1usize as _);
        let b = HWND(2usize as _);
        let mut requests = Requests::default();
        let open = |client| Request::Open {
            client,
            layer: Layer::Popup,
            rect: Rect {
                x: 10,
                y: 20,
                width: 300,
                height: 200,
            },
            radius: 12.0,
            material: Material::CLEAR,
        };
        requests.remember(open(a));
        requests.remember(open(b));
        let style = Style {
            brightness: 1.5,
            ..Style::DEFAULT
        };
        requests.remember(Request::Style { client: a, style });
        // Device lost: runtime objects are gone; requests keep arriving during backoff.
        let moved = Rect {
            x: 200,
            y: 100,
            width: 400,
            height: 300,
        };
        requests.remember(Request::Place {
            client: a,
            rect: moved,
            radius: 20.0,
        });
        requests.remember(Request::Close { client: b });
        let live = requests.live(|_| true);
        assert_eq!(live.len(), 1);
        assert_eq!(
            (live[0].rect, live[0].radius, live[0].style),
            (moved, 20.0, style)
        );
        let mut reopened = Requests::default();
        reopened.remember(live[0].open());
        reopened.remember(Request::Style {
            client: a,
            style: live[0].style,
        });
        assert_eq!(reopened.live(|_| true), live);
        assert!(
            requests.live(|_| false).is_empty(),
            "destroyed clients stay closed"
        );
    }

    #[test]
    fn recognizes_context_wrapped_device_errors_only() {
        for code in [0x887A0005u32, 0x887A0006, 0x887A0007, 0x887A0020] {
            let e = anyhow::Error::from(windows::core::Error::from_hresult(
                windows::core::HRESULT(code as i32),
            ))
            .context("overlay update");
            assert!(device_lost(&e));
        }
        assert!(!device_lost(&anyhow::anyhow!("window destroyed")));
    }
}
