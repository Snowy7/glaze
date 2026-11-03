//! Pure math for scene wallpapers: bounds, the orbit/parallax camera and
//! color encoding. No GPU types, so it is all unit-testable.

use std::time::Duration;

use glam::camera::rh::proj::directx::perspective;
use glam::camera::rh::view::look_at_mat4;
use glam::{Mat4, Vec2, Vec3};

/// Vertical field of view of the scene camera.
const FOV_Y: f32 = 40.0 * std::f32::consts::PI / 180.0;
/// Camera height above the scene's equator, as an angle.
const ELEVATION: f32 = 14.0 * std::f32::consts::PI / 180.0;
/// Fraction of the exact "sphere touches the frame" distance the camera sits
/// at. Below 1 the scene fills more of the screen (the bounding sphere is
/// conservative, so the actual silhouette still has slack).
const FRAMING: f32 = 0.66;

/// A bounding sphere.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sphere {
    pub center: Vec3,
    pub radius: f32,
}

impl Sphere {
    /// Centered on the points' bounding box, reaching the farthest point.
    /// `None` for no points.
    pub fn around(points: impl Iterator<Item = Vec3> + Clone) -> Option<Self> {
        let (min, max) = points.clone().fold(
            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
            |(min, max), p| (min.min(p), max.max(p)),
        );
        let center = (min + max) / 2.0;
        let radius = points.map(|p| p.distance(center)).max_by(f32::total_cmp)?;
        Some(Self { center, radius })
    }
}

/// The camera for one frame.
#[derive(Debug, Clone, Copy)]
pub struct View {
    pub eye: Vec3,
    pub view_proj: Mat4,
    /// Distance from the eye to the nearest possible scene point (fog starts here).
    pub nearest: f32,
}

/// Distance at which a sphere of `radius` exactly fits the narrower of the
/// vertical and horizontal fields of view.
fn fit_distance(radius: f32, aspect: f32) -> f32 {
    let half_h = (FOV_Y / 2.0).tan() * aspect;
    let half = (FOV_Y / 2.0).min(half_h.atan());
    radius / half.sin()
}

/// Camera orbiting the sphere's center.
///
/// `azimuth` is the orbit angle in radians (0 = looking down -Z, from +Z).
/// `lean` is the mouse in -1..1 (x right, y up); the eye shifts toward it
/// by `parallax * distance` while still looking at the center.
pub fn camera(sphere: Sphere, aspect: f32, azimuth: f32, lean: Vec2, parallax: f32) -> View {
    let distance = fit_distance(sphere.radius, aspect) * FRAMING;
    let dir = Vec3::new(
        ELEVATION.cos() * azimuth.sin(),
        ELEVATION.sin(),
        ELEVATION.cos() * azimuth.cos(),
    );
    let right = Vec3::Y.cross(dir).normalize();
    let up = dir.cross(right);
    let eye = sphere.center + dir * distance + (right * lean.x + up * lean.y) * parallax * distance;

    let from_center = eye.distance(sphere.center);
    let near = (from_center - sphere.radius * 1.05).max(from_center * 0.05);
    let far = from_center + sphere.radius * 1.05;
    let view = look_at_mat4(eye, sphere.center, Vec3::Y);
    let proj = perspective(FOV_Y, aspect, near, far);
    View {
        eye,
        view_proj: proj * view,
        nearest: (from_center - sphere.radius).max(0.0),
    }
}

/// Orbit angle in radians after `clock`, wrapped to one turn. Computed in
/// f64 so the camera stays smooth after days of uptime.
pub fn azimuth(degrees_per_second: f32, clock: Duration) -> f32 {
    ((f64::from(degrees_per_second) * clock.as_secs_f64()) % 360.0).to_radians() as f32
}

/// Moves `current` toward `target`, exponentially at `rate` per second.
/// Frame-rate independent: two half steps equal one whole step.
pub fn approach(current: Vec2, target: Vec2, dt: f32, rate: f32) -> Vec2 {
    current.lerp(target, 1.0 - (-rate * dt.max(0.0)).exp())
}

/// Linear light to sRGB-encoded (both 0..1). The shader does the same on
/// the GPU; the CPU needs it for clear colors on the non-sRGB swapchain.
pub fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPHERE: Sphere = Sphere {
        center: Vec3::new(1.0, 2.0, 3.0),
        radius: 5.0,
    };

    fn ndc(view: &View, p: Vec3) -> Vec3 {
        view.view_proj.project_point3(p)
    }

    #[test]
    fn sphere_reaches_the_farthest_point_from_the_box_center() {
        let points = [
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(3.0, 0.0, 0.0),
            Vec3::new(1.0, 2.0, 0.0),
        ];
        let s = Sphere::around(points.into_iter()).unwrap();
        assert_eq!(s.center, Vec3::new(1.0, 1.0, 0.0));
        assert!((s.radius - 5.0_f32.sqrt()).abs() < 1e-6);
        assert!(Sphere::around(std::iter::empty()).is_none());
    }

    #[test]
    fn fit_distance_uses_the_narrower_field_of_view() {
        // Landscape and square: vertical fov limits. Portrait: horizontal does.
        let landscape = fit_distance(1.0, 16.0 / 9.0);
        assert!((landscape - 1.0 / (FOV_Y / 2.0).sin()).abs() < 1e-5);
        assert_eq!(fit_distance(1.0, 1.0), landscape);
        assert!(fit_distance(1.0, 0.5) > landscape);
    }

    #[test]
    fn orbit_circles_the_center_and_keeps_it_framed() {
        let at_zero = camera(SPHERE, 1.5, 0.0, Vec2::ZERO, 0.0);
        let quarter = camera(SPHERE, 1.5, std::f32::consts::FRAC_PI_2, Vec2::ZERO, 0.0);
        // Same distance and height; the eye swings from +Z to +X.
        let (a, b) = (at_zero.eye - SPHERE.center, quarter.eye - SPHERE.center);
        assert!((a.length() - b.length()).abs() < 1e-4 && (a.y - b.y).abs() < 1e-4);
        assert!(a.z > 0.0 && a.x.abs() < 1e-4);
        assert!(b.x > 0.0 && b.z.abs() < 1e-4);
        // The center sits mid-screen, in front of the near plane.
        let c = ndc(&quarter, SPHERE.center);
        assert!(c.x.abs() < 1e-4 && c.y.abs() < 1e-4 && (0.0..1.0).contains(&c.z));
    }

    #[test]
    fn parallax_slides_the_eye_across_the_view_not_along_it() {
        let still = camera(SPHERE, 1.5, 0.0, Vec2::ZERO, 0.2);
        let leaned = camera(SPHERE, 1.5, 0.0, Vec2::X, 0.2);
        let shift = leaned.eye - still.eye;
        let distance = (still.eye - SPHERE.center).length();
        assert!((shift.x - 0.2 * distance).abs() < 1e-4);
        // Perpendicular to the original view direction.
        assert!(shift.dot(still.eye - SPHERE.center).abs() < 1e-3);
        // Still looking at the center.
        let c = ndc(&leaned, SPHERE.center);
        assert!(c.x.abs() < 1e-4 && c.y.abs() < 1e-4);
    }

    #[test]
    fn the_whole_sphere_stays_inside_the_depth_range() {
        let view = camera(SPHERE, 1.78, 1.0, Vec2::new(1.0, -1.0), 0.3);
        let to_center = (SPHERE.center - view.eye).normalize();
        for sign in [-1.0, 1.0] {
            let z = ndc(&view, SPHERE.center + to_center * SPHERE.radius * sign).z;
            assert!((0.0..=1.0).contains(&z), "z = {z}");
        }
    }

    #[test]
    fn azimuth_wraps_and_stays_precise_after_long_uptime() {
        let quarter = std::f32::consts::FRAC_PI_2;
        assert!((azimuth(4.0, Duration::from_secs(90)) - 0.0).abs() < 1e-5);
        assert!((azimuth(4.0, Duration::from_millis(22_500)) - quarter).abs() < 1e-5);
        // 1000 turns plus a quarter at 1 deg/s: an f32 clock would be off by whole degrees.
        let long = Duration::from_secs(360 * 1000 + 90);
        assert!((azimuth(1.0, long) - quarter).abs() < 1e-5);
    }

    #[test]
    fn approach_is_frame_rate_independent() {
        let (from, to) = (Vec2::ZERO, Vec2::new(1.0, -1.0));
        let whole = approach(from, to, 0.2, 6.0);
        let halves = approach(approach(from, to, 0.1, 6.0), to, 0.1, 6.0);
        assert!((whole - halves).length() < 1e-6);
        assert_eq!(approach(from, to, 0.0, 6.0), from);
        assert!((approach(from, to, 100.0, 6.0) - to).length() < 1e-6);
    }

    #[test]
    fn srgb_encoding_matches_the_standard_curve() {
        assert_eq!(linear_to_srgb(0.0), 0.0);
        assert!((linear_to_srgb(1.0) - 1.0).abs() < 1e-6);
        // Linear segment and the well-known mid-gray.
        assert!((linear_to_srgb(0.002) - 0.002 * 12.92).abs() < 1e-7);
        assert!((linear_to_srgb(0.214_04) - 0.5).abs() < 1e-3);
    }
}
