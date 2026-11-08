//! A small software renderer: analytic surfaces, perspective, a depth buffer and light.
//!
//! Coordinates use +y upwards and -z towards the viewer. Terminal characters are
//! roughly twice as tall as they are wide; square subpixels use an aspect of one.

use std::f64::consts::PI;

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
            offset: Vec3::new(1.3 * (time * 1.2).sin(), 0.20 * (time * 4.8).sin(), 0.0),
        }
    }

    fn unrotate(self, point: Vec3) -> Vec3 {
        let (sy, cy) = self.yaw;
        let (sr, cr) = self.roll;
        let x = point.x * cr + point.y * sr;
        let y = -point.x * sr + point.y * cr;
        Vec3::new(x * cy - point.z * sy, y, x * sy + point.z * cy)
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
    Rim,
    Underside,
    Groove,
    Eye,
    Pupil,
    ClosedEye,
}

/// Reusable ASCII canvas. No terminal control codes are emitted by this module.
pub struct Renderer {
    width: usize,
    height: usize,
    scale: f64,
    vertical_scale: f64,
    pixels: Vec<u8>,
    depth: Vec<f64>,
    pose: Pose,
    light: Vec3,
}

impl Renderer {
    pub fn new(width: usize, height: usize) -> Self {
        Self::with_aspect(width, height, 0.5)
    }

    /// Use 0.5 for terminal characters, or 1.0 for square subpixels.
    pub fn with_aspect(width: usize, height: usize, vertical_scale: f64) -> Self {
        let vertical_scale = if vertical_scale.is_finite() && vertical_scale > 0.0 {
            vertical_scale
        } else {
            0.5
        };
        Self {
            width,
            height,
            scale: ((width.saturating_sub(2) as f64) / 20.5)
                .min((height.saturating_sub(2) as f64) / (12.0 * vertical_scale)),
            vertical_scale,
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

    fn store(&mut self, index: usize, depth: f64, normal: Vec3, material: Material) {
        if depth <= 1.0 || !depth.is_finite() || depth >= self.depth[index] {
            return;
        }
        let shade = match material {
            Material::Shell | Material::Rim | Material::Underside | Material::Groove => {
                let diffuse = self
                    .pose
                    .rotate(normal)
                    .normalized()
                    .dot(self.light)
                    .max(0.0);
                let brightness = match material {
                    Material::Rim => 0.32 + 0.68 * diffuse,
                    Material::Underside => 0.12 + 0.45 * diffuse,
                    Material::Groove => 0.08 + 0.10 * diffuse,
                    _ => 0.19 + 0.81 * diffuse,
                };
                SHADES[(brightness * (SHADES.len() - 1) as f64).round() as usize]
            }
            Material::Eye => b'O',
            Material::Pupil => b'o',
            Material::ClosedEye => b'_',
        };
        self.depth[index] = depth;
        self.pixels[index] = shade;
    }

    /// Conservative projected bounds of a world-space box.
    fn bounds(&self, low: Vec3, high: Vec3) -> Option<(usize, usize, usize, usize)> {
        if self.width == 0 || self.height == 0 || self.scale <= 0.0 {
            return None;
        }
        let (mut left, mut right, mut top, mut bottom) = (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        );
        for x in [low.x, high.x] {
            for y in [low.y, high.y] {
                for z in [low.z, high.z] {
                    let perspective = 28.0 / (z + 28.0).max(1.0);
                    let sx = self.width as f64 * 0.5 + x * self.scale * perspective;
                    let sy = self.height as f64 * 0.5
                        - (y - 0.9) * self.scale * self.vertical_scale * perspective;
                    left = left.min(sx);
                    right = right.max(sx);
                    top = top.min(sy);
                    bottom = bottom.max(sy);
                }
            }
        }
        if right < 0.0 || bottom < 0.0 || left >= self.width as f64 || top >= self.height as f64 {
            return None;
        }
        Some((
            left.floor().max(0.0) as usize,
            right.ceil().min((self.width - 1) as f64) as usize,
            top.floor().max(0.0) as usize,
            bottom.ceil().min((self.height - 1) as f64) as usize,
        ))
    }

    fn world(&self, point: Vec3) -> Vec3 {
        self.pose.rotate(point).plus(self.pose.offset)
    }

    fn camera(&self) -> Vec3 {
        self.pose
            .unrotate(Vec3::new(0.0, 0.9, -28.0).minus(self.pose.offset))
    }

    fn ray(&self, x: usize, y: usize) -> Vec3 {
        self.pose.unrotate(Vec3::new(
            (x as f64 - self.width as f64 * 0.5) / (self.scale * 28.0),
            -(y as f64 - self.height as f64 * 0.5) / (self.scale * self.vertical_scale * 28.0),
            1.0,
        ))
    }

    /// Analytic surface intersections fill every covered cell without sampling holes.
    fn ellipsoid(&mut self, center: Vec3, radius: Vec3, material: Material) {
        let largest = radius.x.max(radius.y).max(radius.z);
        let extent = Vec3::new(largest, largest, largest);
        let world = self.world(center);
        let Some((left, right, top, bottom)) = self.bounds(world.minus(extent), world.plus(extent))
        else {
            return;
        };
        let origin = self.camera().minus(center);
        let inverse = Vec3::new(1.0 / radius.x, 1.0 / radius.y, 1.0 / radius.z);
        let scaled_origin = Vec3::new(
            origin.x * inverse.x,
            origin.y * inverse.y,
            origin.z * inverse.z,
        );
        for y in top..=bottom {
            for x in left..=right {
                let direction = self.ray(x, y);
                let scaled_direction = Vec3::new(
                    direction.x * inverse.x,
                    direction.y * inverse.y,
                    direction.z * inverse.z,
                );
                let a = scaled_direction.dot(scaled_direction);
                let b = scaled_origin.dot(scaled_direction);
                let c = scaled_origin.dot(scaled_origin) - 1.0;
                let discriminant = b * b - a * c;
                if discriminant < 0.0 {
                    continue;
                }
                let depth = (-b - discriminant.sqrt()) / a;
                let hit = origin.plus(direction.scaled(depth));
                let normal = Vec3::new(
                    hit.x * inverse.x * inverse.x,
                    hit.y * inverse.y * inverse.y,
                    hit.z * inverse.z * inverse.z,
                );
                self.store(y * self.width + x, depth, normal, material);
            }
        }
    }

    fn limb(&mut self, start: Vec3, end: Vec3, start_radius: f64, end_radius: f64) {
        // Keep narrow feet and fingertips connected on the coarse character grid.
        // Finer square subpixels need almost no thickness adjustment.
        let minimum = (0.54 / (self.scale * self.vertical_scale)).min(0.25);
        self.limb_material(
            start,
            end,
            start_radius.max(minimum),
            end_radius.max(minimum),
            Material::Shell,
        );
    }

    /// Ray-intersect a tapered segment, then round its joints with small spheres.
    fn limb_material(
        &mut self,
        start: Vec3,
        end: Vec3,
        start_radius: f64,
        end_radius: f64,
        material: Material,
    ) {
        let delta = end.minus(start);
        let length = delta.length();
        if length < f64::EPSILON {
            return;
        }
        let axis = delta.scaled(1.0 / length);
        let slope = (end_radius - start_radius) / length;
        let radius = start_radius.max(end_radius);
        let a = self.world(start);
        let b = self.world(end);
        let low = Vec3::new(
            a.x.min(b.x) - radius,
            a.y.min(b.y) - radius,
            a.z.min(b.z) - radius,
        );
        let high = Vec3::new(
            a.x.max(b.x) + radius,
            a.y.max(b.y) + radius,
            a.z.max(b.z) + radius,
        );
        if let Some((left, right, top, bottom)) = self.bounds(low, high) {
            let origin = self.camera().minus(start);
            let along_origin = origin.dot(axis);
            let across_origin = origin.minus(axis.scaled(along_origin));
            let origin_radius = start_radius + slope * along_origin;
            for y in top..=bottom {
                for x in left..=right {
                    let direction = self.ray(x, y);
                    let along_direction = direction.dot(axis);
                    let across_direction = direction.minus(axis.scaled(along_direction));
                    let radius_direction = slope * along_direction;
                    let qa = across_direction.dot(across_direction)
                        - radius_direction * radius_direction;
                    let qb = across_origin.dot(across_direction) - origin_radius * radius_direction;
                    let qc = across_origin.dot(across_origin) - origin_radius * origin_radius;
                    let discriminant = qb * qb - qa * qc;
                    if qa.abs() < 1e-12 || discriminant < 0.0 {
                        continue;
                    }
                    let root = discriminant.sqrt();
                    let mut depths = [(-qb - root) / qa, (-qb + root) / qa];
                    depths.sort_by(f64::total_cmp);
                    for depth in depths {
                        let along = along_origin + depth * along_direction;
                        if !(0.0..=length).contains(&along) || depth <= 1.0 {
                            continue;
                        }
                        let hit = origin.plus(direction.scaled(depth));
                        let radial = hit.minus(axis.scaled(along));
                        let normal =
                            radial.minus(axis.scaled(slope * (start_radius + slope * along)));
                        self.store(y * self.width + x, depth, normal, material);
                        break;
                    }
                }
            }
        }
        for (center, radius) in [(start, start_radius), (end, end_radius)] {
            self.ellipsoid(center, Vec3::new(radius, radius, radius), material);
        }
    }

    fn crab(&mut self, time: f64) {
        for side in [-1.0, 1.0] {
            for leg in 0..4 {
                let [hip, knee, ankle, foot] = leg_pose(time, side, leg);
                self.limb(hip, knee, 0.28, 0.22);
                self.ellipsoid(knee, Vec3::new(0.29, 0.26, 0.27), Material::Rim);
                self.limb(knee, ankle, 0.20, 0.13);
                self.limb(ankle, foot, 0.13, 0.045);
            }

            let wave = (time * 2.4 + side * 1.1).sin();
            let shoulder = Vec3::new(side * 2.7, 0.95, -0.10);
            let elbow = Vec3::new(side * 4.1, 1.2 + wave * 0.35, -0.35);
            let palm = Vec3::new(side * 5.05, 2.65 + wave * 0.6, -0.65);
            self.limb(shoulder, elbow, 0.42, 0.34);
            self.ellipsoid(elbow, Vec3::new(0.43, 0.40, 0.38), Material::Rim);
            self.limb(elbow, palm, 0.34, 0.46);
            self.ellipsoid(palm, Vec3::new(0.73, 0.79, 0.56), Material::Shell);
            let opening = 0.68 * (0.5 + 0.5 * (time * 2.8 + side * 0.95).sin());
            for jaw in [-1.0, 1.0] {
                let [hinge, bend, tip] = pincer_jaw(palm, jaw, opening);
                self.ellipsoid(hinge, Vec3::new(0.34, 0.32, 0.34), Material::Rim);
                self.limb(hinge, bend, 0.34, 0.24);
                self.limb(bend, tip, 0.24, 0.045);
            }
        }

        // The lower plate remains visible beneath the domed carapace.
        self.ellipsoid(
            Vec3::new(0.0, -0.48, 0.05),
            Vec3::new(3.62, 1.57, 1.25),
            Material::Underside,
        );
        self.ellipsoid(
            Vec3::new(0.0, 0.12, -0.08),
            Vec3::new(3.50, 1.72, 1.40),
            Material::Shell,
        );

        // A raised lower rim follows the shell silhouette, not a floating line.
        let mut previous = None;
        for step in 0..=40 {
            let angle = PI + PI * step as f64 / 40.0;
            let point = Vec3::new(3.50 * angle.cos(), 0.12 + 1.72 * angle.sin(), -0.12);
            if let Some(start) = previous {
                self.limb_material(start, point, 0.085, 0.085, Material::Rim);
            }
            previous = Some(point);
        }
        // Short grooves and the lower seam sit on the shell's curved front.
        for side in [-1.0, 1.0] {
            let mut previous = None;
            for step in 0..=10 {
                let t = step as f64 / 10.0;
                let point = shell_front(side * (1.55 + t * 1.38), 0.68 - t * 0.90);
                if let Some(start) = previous {
                    self.limb_material(start, point, 0.047, 0.047, Material::Groove);
                }
                previous = Some(point);
            }
        }
        let mut previous = None;
        for step in 0..=36 {
            let x = -3.0 + 6.0 * step as f64 / 36.0;
            let y = -0.93 - 0.28 * (1.0 - (x / 3.2).powi(2)).sqrt();
            let point = shell_front(x, y);
            if let Some(start) = previous {
                self.limb_material(start, point, 0.055, 0.055, Material::Groove);
            }
            previous = Some(point);
        }

        for side in [-1.0, 1.0] {
            let base = Vec3::new(side * 1.25, 1.2, -0.45);
            // Stalks waggle out of phase, independently of the shell and claws.
            let eye = Vec3::new(
                side * 1.38 + 0.27 * (time * 2.2 + side * 1.1).sin(),
                2.65 + 0.32 * (time * 2.8 + side * 0.9).sin(),
                -0.75,
            );
            self.limb(base, eye, 0.17, 0.17);
            let blink = (time - side * 0.025).rem_euclid(5.2);
            if (4.0..4.18).contains(&blink) {
                self.ellipsoid(eye, Vec3::new(0.55, 0.09, 0.44), Material::ClosedEye);
            } else {
                self.ellipsoid(eye, Vec3::new(0.54, 0.58, 0.45), Material::Eye);
                self.ellipsoid(
                    eye.plus(Vec3::new(0.14 * (time * 1.5).sin(), 0.0, -0.40)),
                    Vec3::new(0.19, 0.28, 0.1),
                    Material::Pupil,
                );
            }
        }
    }
}

/// Three articulated segments give each leg a distinct knee and pointed foot.
fn leg_pose(time: f64, side: f64, leg: usize) -> [Vec3; 4] {
    let index = leg as f64;
    let phase = time * 4.8 + index * PI * 0.75 + side * PI * 0.5;
    let swing = phase.sin();
    let lift = phase.cos().max(0.0) * 0.50;
    [
        Vec3::new(side * 2.85, 0.55 - index * 0.46, 0.40 + index * 0.12),
        Vec3::new(
            side * (4.7 + index * 0.30),
            1.03 - index * 0.88 + swing * 0.16,
            0.12 + index * 0.22,
        ),
        Vec3::new(
            side * (6.2 + index * 0.18 + swing * 0.25),
            0.20 - index * 0.90 + lift * 0.5,
            -0.28 + index * 0.10,
        ),
        Vec3::new(
            side * (7.45 - index * 0.13 + swing * 0.38),
            -0.24 - index * 1.02 + lift,
            -0.55 + index * 0.12,
        ),
    ]
}

/// Both jaw tips meet when opening is zero; each rotates about its own hinge.
fn pincer_jaw(palm: Vec3, jaw: f64, opening: f64) -> [Vec3; 3] {
    let hinge = palm.plus(Vec3::new(jaw * 0.43, 0.28, 0.0));
    let angle = -jaw * opening;
    let (s, c) = angle.sin_cos();
    let rotate = |point: Vec3| {
        hinge.plus(Vec3::new(
            point.x * c - point.y * s,
            point.x * s + point.y * c,
            point.z,
        ))
    };
    [
        hinge,
        rotate(Vec3::new(jaw * 0.38, 0.85, 0.0)),
        rotate(Vec3::new(-jaw * 0.43, 1.55, 0.0)),
    ]
}

fn shell_front(x: f64, y: f64) -> Vec3 {
    let inside = 1.0 - (x / 3.50).powi(2) - ((y - 0.12) / 1.72).powi(2);
    Vec3::new(x, y, -0.08 - 1.40 * inside.max(0.0).sqrt() - 0.035)
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
                        > width / 2
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
        assert!(first.matches('o').count() >= 2);
        assert_ne!(first, renderer.render(0.5));
        assert_ne!(renderer.render(0.5), renderer.render(1.0));

        let eye_height_difference = |frame: &str| {
            let eyes: Vec<_> = frame
                .lines()
                .enumerate()
                .flat_map(|(y, line)| line.bytes().enumerate().map(move |(x, byte)| (x, y, byte)))
                .filter(|(_, _, byte)| matches!(byte, b'O' | b'o'))
                .collect();
            let middle = (eyes.iter().map(|p| p.0).min().unwrap()
                + eyes.iter().map(|p| p.0).max().unwrap()) as f64
                / 2.0;
            let mean_y = |left: bool| {
                let group: Vec<_> = eyes
                    .iter()
                    .filter(|p| ((p.0 as f64) < middle) == left)
                    .collect();
                group.iter().map(|p| p.1 as f64).sum::<f64>() / group.len() as f64
            };
            mean_y(true) - mean_y(false)
        };
        // Eye stalks change their height relative to each other as they waggle.
        assert!(
            (eye_height_difference(&first) - eye_height_difference(&renderer.render(0.7))).abs()
                > 1.0
        );
    }

    #[test]
    fn eyes_blink_and_scuttle_stays_inside_the_canvas() {
        let mut renderer = Renderer::new(100, 32);
        let awake = renderer.render(0.0);
        assert!(awake.contains('O') && awake.contains('o'));
        let blinking = renderer.render(4.08);
        assert!(blinking.contains('_'));
        assert!(!blinking.contains('O') && !blinking.contains('o'));
        let awake_again = renderer.render(4.3);
        assert!(awake_again.contains('O') && awake_again.contains('o'));

        let mut leftmost = usize::MAX;
        let mut rightmost = 0;
        for index in 0..120 {
            let frame = renderer.render(index as f64 / 10.0);
            let occupied: Vec<_> = frame
                .lines()
                .flat_map(|line| line.bytes().enumerate())
                .filter_map(|(x, byte)| (byte != b' ').then_some(x))
                .collect();
            let left = *occupied.iter().min().unwrap();
            let right = *occupied.iter().max().unwrap();
            assert!(left > 0 && right < 99, "crab clipped at frame {index}");
            leftmost = leftmost.min(left);
            rightmost = rightmost.max(left);
        }
        // The complete silhouette visibly travels across the terminal.
        assert!(rightmost - leftmost >= 8);
    }

    #[test]
    fn hinged_jaws_close_and_open_at_the_tips() {
        let palm = Vec3::new(0.0, 0.0, 0.0);
        let closed_left = pincer_jaw(palm, -1.0, 0.0);
        let closed_right = pincer_jaw(palm, 1.0, 0.0);
        let open_left = pincer_jaw(palm, -1.0, 0.68);
        let open_right = pincer_jaw(palm, 1.0, 0.68);
        assert!(closed_left[2].minus(closed_right[2]).length() < 1e-9);
        assert!(open_left[2].minus(open_right[2]).length() > 2.0);
        assert!(open_left[0].minus(closed_left[0]).length() < 1e-9);
        assert!(open_right[0].minus(closed_right[0]).length() < 1e-9);
    }

    #[test]
    fn eight_legs_step_independently_with_visible_joints() {
        for side in [-1.0, 1.0] {
            for leg in 0..4 {
                let first = leg_pose(0.0, side, leg);
                let next = leg_pose(0.25, side, leg);
                for pair in first.windows(2) {
                    assert!(pair[1].minus(pair[0]).length() > 0.5);
                }
                assert!(first[3].minus(next[3]).length() > 0.1);
                if leg > 0 {
                    assert!((first[3].y - leg_pose(0.0, side, leg - 1)[3].y).abs() > 0.45);
                }
            }
        }
    }

    #[test]
    fn square_subpixels_fill_surfaces_without_sampling_holes() {
        let mut renderer = Renderer::with_aspect(400, 256, 1.0);
        renderer.ellipsoid(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(3.5, 1.7, 1.4),
            Material::Shell,
        );
        for row in renderer.pixels.chunks(renderer.width) {
            if let (Some(left), Some(right)) = (
                row.iter().position(|c| *c != b' '),
                row.iter().rposition(|c| *c != b' '),
            ) {
                assert!(row[left..=right].iter().all(|c| *c != b' '));
            }
        }
        let frame = renderer.render(1.5);
        assert_eq!(frame.lines().count(), 256);
        assert!(frame.lines().all(|line| line.len() == 400));
        assert!(renderer
            .depth
            .iter()
            .all(|depth| depth.is_infinite() || depth.is_finite() && *depth > 1.0));
    }

    #[test]
    fn invalid_time_is_safe() {
        let mut renderer = Renderer::new(80, 24);
        let first = renderer.render(0.0);
        assert_eq!(first, renderer.render(f64::NAN));
        assert_eq!(first, renderer.render(f64::INFINITY));
    }
}
