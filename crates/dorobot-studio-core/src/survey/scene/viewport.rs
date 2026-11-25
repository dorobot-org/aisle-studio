//! Metric floor grid and depth-tested cuboids shared by the editor's two projections.
use super::{Revision, Scene};
use crate::survey::asset::Asset;
use image::{Rgb, RgbImage};
use std::collections::HashMap;

pub const WIDTH: u32 = 800;
pub const HEIGHT: u32 = 448;

#[derive(Clone, Copy)]
pub struct Camera {
    azimuth: f64,
    elevation: f64,
    scale: f64,
}

impl Camera {
    pub fn new(room: [f64; 2], top: bool) -> Self {
        let azimuth: f64 = if top { 0. } else { 0.55 };
        let elevation: f64 = if top {
            std::f64::consts::FRAC_PI_2
        } else {
            0.84
        };
        let (s, c) = azimuth.sin_cos();
        let projected_w = room[0] * c.abs() + room[1] * s.abs();
        let projected_h = (room[0] * s.abs() + room[1] * c.abs()) * elevation.sin();
        let scale = (720. / projected_w).min(320. / projected_h);
        Self {
            azimuth,
            elevation,
            scale,
        }
    }

    fn project(&self, [x, y, z]: [f64; 3]) -> [f64; 3] {
        let (s, c) = self.azimuth.sin_cos();
        let (v, w) = self.elevation.sin_cos();
        let away = x * s + y * c;
        [
            400. + self.scale * (x * c - y * s),
            247. - self.scale * (away * v + z * w),
            -away * w + z * v,
        ]
    }

    pub fn floor(&self, px: f64, py: f64, height: f64) -> [f64; 2] {
        let (s, c) = self.azimuth.sin_cos();
        let (v, w) = self.elevation.sin_cos();
        let right = (px - 400.) / self.scale;
        let away = ((247. - py) / self.scale - height * w) / v;
        [right * c + away * s, -right * s + away * c]
    }
}

pub struct Viewport {
    pub image: RgbImage,
    ids: Vec<Option<usize>>,
}

impl Viewport {
    pub fn at(&self, x: f64, y: f64) -> Option<usize> {
        if !(0.0..WIDTH as f64).contains(&x) || !(0.0..HEIGHT as f64).contains(&y) {
            return None;
        }
        self.ids[y as usize * WIDTH as usize + x as usize]
    }

    pub fn render(
        scene: &Scene,
        assets: &HashMap<Revision, Asset>,
        camera: Camera,
        selected: Option<&str>,
    ) -> Self {
        let mut image = RgbImage::from_pixel(WIDTH, HEIGHT, Rgb([238, 241, 244]));
        let mut ids = vec![None; (WIDTH * HEIGHT) as usize];
        let mut depths = vec![f64::NEG_INFINITY; ids.len()];
        let [hx, hy] = scene.room_m.map(|m| m / 2.);
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let [u, v] = camera.floor(x as f64 + 0.5, y as f64 + 0.5, 0.);
                if u.abs() > hx || v.abs() > hy {
                    continue;
                }
                let line = 0.65 / camera.scale;
                let distance = |d: f64| (d - (d * 2.).round() / 2.).abs();
                let color = if hx - u.abs() < line * 2. || hy - v.abs() < line * 2. {
                    [115, 132, 143]
                } else if v.abs() < line {
                    [185, 102, 101]
                } else if u.abs() < line {
                    [69, 143, 135]
                } else if distance(u) < line || distance(v) < line {
                    [200, 210, 215]
                } else {
                    [250, 251, 252]
                };
                image.put_pixel(x, y, Rgb(color));
            }
        }
        for (index, item) in scene.instances.iter().enumerate() {
            let active = Some(item.id.as_str()) == selected;
            let base: [u8; 3] = if active {
                [50, 148, 195]
            } else {
                [136, 158, 170]
            };
            let (sa, ca) = item.yaw_deg.to_radians().sin_cos();
            let [tx, ty, tz] = item.position_m;
            let Some(asset) = assets
                .get(&item.asset)
                .filter(|a| !a.geometry.colliders.is_empty())
            else {
                // A cross locates unmeasured assets without inventing a physical extent.
                let [u, v, z] = camera.project(item.position_m);
                for d in -7_i32..=7 {
                    for (i, j) in [(u as i32 + d, v as i32), (u as i32, v as i32 + d)] {
                        if i >= 0 && j >= 0 && i < WIDTH as i32 && j < HEIGHT as i32 {
                            let n = j as usize * WIDTH as usize + i as usize;
                            if z >= depths[n] {
                                ids[n] = Some(index);
                                depths[n] = z;
                                image.put_pixel(
                                    i as u32,
                                    j as u32,
                                    Rgb(if active {
                                        [20, 115, 190]
                                    } else {
                                        [192, 122, 58]
                                    }),
                                );
                            }
                        }
                    }
                }
                continue;
            };
            for body in &asset.geometry.colliders {
                let points: [[f64; 3]; 8] = std::array::from_fn(|i| {
                    let [x, y, z]: [f64; 3] = std::array::from_fn(|k| {
                        body.center_m[k]
                            + body.size_m[k] * if i & (1 << k) == 0 { -0.5 } else { 0.5 }
                    });
                    camera.project([tx + ca * x - sa * y, ty + sa * x + ca * y, tz + z])
                });
                for (face, shade) in [
                    ([0, 1, 3, 2], 0.65),
                    ([4, 5, 7, 6], 1.18),
                    ([0, 1, 5, 4], 0.88),
                    ([2, 3, 7, 6], 0.86),
                    ([0, 2, 6, 4], 0.72),
                    ([1, 3, 7, 5], 0.81),
                ] {
                    let color = Rgb(base.map(|v| (v as f64 * shade).min(255.) as u8));
                    for tri in [[face[0], face[1], face[2]], [face[0], face[2], face[3]]] {
                        let [a, b, c] = tri.map(|i| points[i]);
                        let cross = |p: [f64; 3], q: [f64; 3], x: f64, y: f64| {
                            (q[0] - p[0]) * (y - p[1]) - (q[1] - p[1]) * (x - p[0])
                        };
                        let area = cross(a, b, c[0], c[1]);
                        if area.abs() < 1e-9 {
                            continue;
                        }
                        let lo = |k: usize| a[k].min(b[k]).min(c[k]).floor().max(0.) as u32;
                        let hi = |k: usize, cap: u32| {
                            (a[k].max(b[k]).max(c[k]).ceil().max(0.) as u32).min(cap)
                        };
                        for y in lo(1)..hi(1, HEIGHT) {
                            for x in lo(0)..hi(0, WIDTH) {
                                let (u, v) = (x as f64 + 0.5, y as f64 + 0.5);
                                let p = cross(b, c, u, v) / area;
                                let q = cross(c, a, u, v) / area;
                                let r = 1. - p - q;
                                if p.min(q).min(r) < -1e-9 {
                                    continue;
                                }
                                let z = p * a[2] + q * b[2] + r * c[2];
                                let n = y as usize * WIDTH as usize + x as usize;
                                if z >= depths[n] {
                                    depths[n] = z;
                                    ids[n] = Some(index);
                                    image.put_pixel(x, y, color);
                                }
                            }
                        }
                    }
                }
            }
        }
        Self { image, ids }
    }
}
