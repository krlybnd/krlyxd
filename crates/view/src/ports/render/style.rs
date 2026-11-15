//! Shared visual rules for exact rays and triangle previews.
use super::{Paint, Projection, Seen, CHANNELS};
use krly3d_core::math::Vec3;

pub const BACKGROUND: [u8; CHANNELS] = [255, 255, 255];
pub const OUTLINE: [u8; CHANNELS] = [24, 32, 48];
const MATERIAL: [f64; CHANNELS] = [168.0, 192.0, 224.0];
const AMBIENT: f64 = 0.35;
const LAMP: [f64; 3] = [-0.35, 0.5, 1.0];

pub fn lamp(projection: &Projection) -> Vec3 {
    (projection.right * LAMP[0] + projection.up * LAMP[1] - projection.toward * LAMP[2]).normalize()
}

pub fn light(facing: &Vec3, lamp: &Vec3) -> f64 {
    AMBIENT + (1.0 - AMBIENT) * facing.dot(lamp).max(0.0)
}

pub fn colour(paint: &[Paint], seen: Seen, light: f64) -> [u8; CHANNELS] {
    let colour = paint
        .get(seen.body)
        .and_then(|paint| paint.of(seen.face))
        .map_or(MATERIAL, |colour| colour.map(f64::from));
    colour.map(|channel| (channel * light).round() as u8)
}

pub fn boundary(normal_dot: f64, depth_step: f64, pixel_size: f64) -> bool {
    normal_dot < 0.94 || depth_step > 3.0 * pixel_size
}
