//! A small software renderer: sampled surfaces, perspective, a depth buffer and light.
//!
//! Coordinates use +y upwards and -z towards the viewer. Terminal characters are
//! roughly twice as tall as they are wide, so projection halves the vertical scale.

use std::f64::consts::{PI, TAU};

const SHADES: &[u8] = b".,-~:;=!*#$@";

#[derive(Clone, Copy, Debug)]
struct Vec3 {
    x: f64,
    y: f64,
    z: f64,
}

impl Vec3 {
    const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    fn plus(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }

    fn minus(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }

    fn scaled(self, amount: f64) -> Self {
        Self::new(self.x * amount, self.y * amount, self.z * amount)
    }

    fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    fn cross(self, other: Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    fn length(self) -> f64 {
        self.dot(self).sqrt()
    }

    fn normalized(self) -> Self {
        let length = self.length();
        if length > f64::EPSILON {
            self.scaled(1.0 / length)
        } else {
            Self::new(0.0, 0.0, 0.0)
        }
    }
}

#[derive(Clone, Copy)]
struct Pose {
    yaw: (f64, f64),
    roll: (f64, f64),
    offset: Vec3,
}

impl Pose {
    fn dancing(time: f64) -> Self {
        Self {
            yaw: (0.28 * (time * 0.85).sin()).sin_cos(),
            roll: (0.07 * (time * 2.4).sin()).sin_cos(),
            offset: Vec3::new(0.45 * (time * 1.2).sin(), 0.20 * (time * 4.8).sin(), 0.0),
        }
    }

    fn rotate(self, point: Vec3) -> Vec3 {
        let (sy, cy) = self.yaw;
        let (sr, cr) = self.roll;
        let x = point.x * cy + point.z * sy;
        let z = -point.x * sy + point.z * cy;
        Vec3::new(x * cr - point.y * sr, x * sr + point.y * cr, z)
    }
}

#[derive(Clone, Copy)]
enum Material {
    Shell,
    Eye,
    Pupil,
}

/// Reusable ASCII canvas. No terminal control codes are emitted by this module.
pub struct Renderer {
    width: usize,
    height: usize,
    scale: f64,
    pixels: Vec<u8>,
    depth: Vec<f64>,
    pose: Pose,
    light: Vec3,
}

impl Renderer {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            scale: ((width.saturating_sub(2) as f64) / 18.5)
                .min((height.saturating_sub(2) as f64) * 2.0 / 12.0),
            pixels: vec![b' '; width * height],
            depth: vec![f64::INFINITY; width * height],
            pose: Pose::dancing(0.0),
            light: Vec3::new(-0.45, 0.8, -1.0).normalized(),
        }
    }

    /// Render one pose. `time` is elapsed seconds, independent of playback speed.
    /// Rows have exactly `width` ASCII cells with newlines only between rows.
    pub fn render(&mut self, time: f64) -> String {
        self.pixels.fill(b' ');
        self.depth.fill(f64::INFINITY);
        // Keep even extremely long sessions within a stable trigonometric range.
        let time = if time.is_finite() {
            time % 100_000.0
        } else {
            0.0
        };
        self.pose = Pose::dancing(time);
        self.crab(time);

        let mut frame = String::with_capacity(self.pixels.len() + self.height);
        for row in 0..self.height {
            if row != 0 {
                frame.push('\n');
            }
            // Every pixel is an ASCII byte from the shade ramp or eye material.
            let start = row * self.width;
            frame.push_str(std::str::from_utf8(&self.pixels[start..start + self.width]).unwrap());
        }
        frame
    }

    fn plot(&mut self, point: Vec3, normal: Vec3, material: Material) {
        let point = self.pose.rotate(point).plus(self.pose.offset);
        let depth = point.z + 28.0;
        if depth <= 1.0 || !depth.is_finite() {
            return;
        }
        let perspective = 28.0 / depth;
        let x = (self.width as f64 * 0.5 + point.x * self.scale * perspective).round();
        let y =
            (self.height as f64 * 0.5 - (point.y - 0.9) * self.scale * 0.5 * perspective).round();
        if x < 0.0 || y < 0.0 || x >= self.width as f64 || y >= self.height as f64 {
            return;
        }
        let index = y as usize * self.width + x as usize;
        if depth >= self.depth[index] {
            return;
        }
        let shade = match material {
            Material::Shell => {
                let normal = self.pose.rotate(normal).normalized();
                let brightness = 0.19 + 0.81 * normal.dot(self.light).max(0.0);
                SHADES[(brightness * (SHADES.len() - 1) as f64).round() as usize]
            }
            Material::Eye => b'O',
            Material::Pupil => b'@',
        };
        self.depth[index] = depth;
        self.pixels[index] = shade;
    }

    fn ellipsoid(&mut self, center: Vec3, radius: Vec3, material: Material) {
        let largest = radius.x.max(radius.y).max(radius.z);
        let rings = ((largest * self.scale * PI).ceil() as usize).clamp(12, 100);
        let slices = rings * 2;
        for ring in 0..=rings {
            let (sp, cp) = (PI * ring as f64 / rings as f64).sin_cos();
            for slice in 0..slices {
                let (st, ct) = (TAU * slice as f64 / slices as f64).sin_cos();
                let unit = Vec3::new(sp * ct, cp, sp * st);
                self.plot(
                    center.plus(Vec3::new(
                        unit.x * radius.x,
                        unit.y * radius.y,
                        unit.z * radius.z,
                    )),
                    Vec3::new(unit.x / radius.x, unit.y / radius.y, unit.z / radius.z),
                    material,
                );
            }
        }
    }

    /// Tapered, round limb with sampled end caps; never a disconnected line of dots.
    fn limb(&mut self, start: Vec3, end: Vec3, start_radius: f64, end_radius: f64) {
        let delta = end.minus(start);
        let axis = delta.normalized();
        let guide = if axis.y.abs() < 0.9 {
            Vec3::new(0.0, 1.0, 0.0)
        } else {
            Vec3::new(1.0, 0.0, 0.0)
        };
        let u = axis.cross(guide).normalized();
        let v = axis.cross(u).normalized();
        let steps = ((delta.length() * self.scale * 2.0).ceil() as usize).max(4);
        let slices =
            ((TAU * start_radius.max(end_radius) * self.scale * 1.5).ceil() as usize).max(12);
        for step in 0..=steps {
            let t = step as f64 / steps as f64;
            let center = start.plus(delta.scaled(t));
            let radius = start_radius + (end_radius - start_radius) * t;
            for slice in 0..slices {
                let (s, c) = (TAU * slice as f64 / slices as f64).sin_cos();
                let radial = u.scaled(c).plus(v.scaled(s));
                let normal = radial.plus(axis.scaled((start_radius - end_radius) / delta.length()));
                self.plot(center.plus(radial.scaled(radius)), normal, Material::Shell);
            }
        }
        for (center, radius) in [(start, start_radius), (end, end_radius)] {
            self.ellipsoid(center, Vec3::new(radius, radius, radius), Material::Shell);
        }
    }

    fn crab(&mut self, time: f64) {
        // Four pairs of legs step in alternating phases. Build them behind the shell.
        for side in [-1.0, 1.0] {
            for leg in 0..4 {
                let index = leg as f64;
                let phase = time * 4.8 + index * PI * 0.75 + side * PI * 0.5;
                let step = phase.sin();
                let lift = phase.cos().max(0.0) * 0.45;
                let hip = Vec3::new(side * 3.0, 0.55 - index * 0.52, 0.5);
                let knee = Vec3::new(
                    side * (5.1 + index * 0.32),
                    0.85 - index * 0.83 + step * 0.18,
                    0.15,
                );
                let foot = Vec3::new(
                    side * (7.5 - index * 0.12 + step * 0.22),
                    -0.35 - index * 0.88 + lift,
                    -0.25,
                );
                self.limb(hip, knee, 0.26, 0.20);
                self.limb(knee, foot, 0.20, 0.055);
            }

            // Each pincer waves from its shoulder while keeping a visible open gap.
            let wave = (time * 2.4 + side * 1.1).sin();
            let shoulder = Vec3::new(side * 2.7, 0.95, -0.10);
            let elbow = Vec3::new(side * 4.25, 1.35 + wave * 0.35, -0.25);
            let palm = Vec3::new(side * 5.05, 2.9 + wave * 0.6, -0.35);
            self.limb(shoulder, elbow, 0.42, 0.34);
            self.limb(elbow, palm, 0.34, 0.46);
            self.ellipsoid(palm, Vec3::new(0.78, 0.8, 0.55), Material::Shell);
            let opening = 0.12 * (time * 3.2 + side).sin();
            let inner = palm.plus(Vec3::new(side * (-0.65 - opening), 1.05, 0.0));
            let outer = palm.plus(Vec3::new(side * (0.82 + opening), 1.05, 0.0));
            self.limb(
                palm.plus(Vec3::new(side * -0.38, 0.27, 0.0)),
                inner,
                0.37,
                0.27,
            );
            self.limb(
                inner,
                palm.plus(Vec3::new(side * -0.25, 1.75, 0.0)),
                0.27,
                0.045,
            );
            self.limb(
                palm.plus(Vec3::new(side * 0.42, 0.15, 0.0)),
                outer,
                0.42,
                0.28,
            );
            self.limb(
                outer,
                palm.plus(Vec3::new(side * 0.35, 1.75, 0.0)),
                0.28,
                0.045,
            );
        }

        // A broad solid carapace, rather than the old torus with a hole through it.
        self.ellipsoid(
            Vec3::new(0.0, -0.1, 0.0),
            Vec3::new(3.55, 1.85, 1.45),
            Material::Shell,
        );

        for side in [-1.0, 1.0] {
            let base = Vec3::new(side * 1.25, 1.2, -0.45);
            let eye = Vec3::new(side * 1.38, 2.5, -0.75);
            self.limb(base, eye, 0.17, 0.17);
            self.ellipsoid(eye, Vec3::new(0.48, 0.52, 0.43), Material::Eye);
            self.ellipsoid(
                eye.plus(Vec3::new(0.03 * (time * 1.2).sin(), 0.0, -0.38)),
                Vec3::new(0.16, 0.24, 0.1),
                Material::Pupil,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_fit_the_canvas_and_remain_visible() {
        for (width, height) in [(20, 8), (80, 24), (100, 32), (160, 48)] {
            let mut renderer = Renderer::new(width, height);
            for time in [0.0, 0.7, 1.5, 3.1, 9.0, 100_000.0] {
                let frame = renderer.render(time);
                assert_eq!(frame.lines().count(), height);
                assert!(frame.lines().all(|line| line.len() == width));
                assert!(frame.is_ascii());
                assert!(!frame.contains('\x1b'));
                assert!(!frame.ends_with('\n'));
                assert!(
                    frame
                        .bytes()
                        .filter(|byte| !byte.is_ascii_whitespace())
                        .count()
                        > width
                );
                assert!(renderer
                    .depth
                    .iter()
                    .all(|z| *z == f64::INFINITY || (*z > 1.0 && z.is_finite())));
            }
        }
    }

    #[test]
    fn crab_dances_and_has_two_visible_eyes() {
        let mut renderer = Renderer::new(100, 32);
        let first = renderer.render(0.0);
        assert!(first.contains('O'));
        assert!(first.matches('@').count() >= 2);
        assert_ne!(first, renderer.render(0.5));
        assert_ne!(renderer.render(0.5), renderer.render(1.0));
    }

    #[test]
    fn invalid_time_is_safe() {
        let mut renderer = Renderer::new(80, 24);
        let first = renderer.render(0.0);
        assert_eq!(first, renderer.render(f64::NAN));
        assert_eq!(first, renderer.render(f64::INFINITY));
    }
}
