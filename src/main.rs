use std::f64::consts::PI;
use std::io::{self, Write};
use std::thread;
use std::time::Duration;

const WIDTH: usize = 120;
const HEIGHT: usize = 30; // Reduced height for better aspect ratio on most terminals
const R1: f64 = 15.0; // Main body radius
const R2: f64 = 8.0;  // Body thickness
const K1: f64 = 40.0; // Distance from viewer to object
const K2: f64 = 60.0; // Distance from viewer to screen
const LIGHT_DIRECTION: [f64; 3] = [0.0, 1.0, -1.0]; // Light direction vector
const SHELL_SEGMENTS: usize = 64; // Higher for smoother shell
const LEG_SEGMENTS: usize = 16;   // Segments per leg
const CLAW_SEGMENTS: usize = 8;   // Segments per claw

// Shade map from darkest to brightest
const SHADE_CHARS: &str = ".,-~:;=!*#$@";

// Structure to represent a 3D point
#[derive(Clone, Copy)]
struct Point3D {
    x: f64,
    y: f64,
    z: f64,
}

impl Point3D {
    fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    fn rotate_x(&mut self, angle: f64) {
        let y = self.y;
        let z = self.z;
        self.y = y * f64::cos(angle) - z * f64::sin(angle);
        self.z = y * f64::sin(angle) + z * f64::cos(angle);
    }

    fn rotate_y(&mut self, angle: f64) {
        let x = self.x;
        let z = self.z;
        self.x = x * f64::cos(angle) - z * f64::sin(angle);
        self.z = x * f64::sin(angle) + z * f64::cos(angle);
    }

    fn rotate_z(&mut self, angle: f64) {
        let x = self.x;
        let y = self.y;
        self.x = x * f64::cos(angle) - y * f64::sin(angle);
        self.y = x * f64::sin(angle) + y * f64::cos(angle);
    }

    fn project(&self) -> (f64, f64, f64) {
        let z = self.z + K1;
        if z <= 0.0 {
            return (0.0, 0.0, -1.0); // Invalid projection
        }
        let scale = K2 / z;
        let x2d = self.x * scale + WIDTH as f64 / 2.0;
        let y2d = self.y * scale + HEIGHT as f64 / 2.0;
        (x2d, y2d, z)
    }

    fn normalize(&self) -> Self {
        const EPSILON: f64 = 1e-10;
        let length = f64::sqrt(self.x * self.x + self.y * self.y + self.z * self.z);
        if length < EPSILON {
            return Self::new(0.0, 0.0, 0.0);
        }
        Self::new(self.x / length, self.y / length, self.z / length)
    }

    fn dot(&self, other: &Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }
}

// Crab model builder
struct CrabModel {
    body_points: Vec<Point3D>,
    body_normals: Vec<Point3D>,
    leg_points: Vec<Vec<Point3D>>, // Multiple legs
    leg_normals: Vec<Vec<Point3D>>,
    claw_points: Vec<Vec<Point3D>>, // Left and right claws
    claw_normals: Vec<Vec<Point3D>>,
    eye_points: Vec<Point3D>,
    eye_normals: Vec<Point3D>,
}

impl CrabModel {
    fn new() -> Self {
        let mut crab = Self {
            body_points: Vec::new(),
            body_normals: Vec::new(),
            leg_points: Vec::new(),
            leg_normals: Vec::new(),
            claw_points: Vec::new(),
            claw_normals: Vec::new(),
            eye_points: Vec::new(),
            eye_normals: Vec::new(),
        };
        crab.generate_body();
        crab.generate_legs();
        crab.generate_claws();
        crab.generate_eyes();
        crab
    }

    fn generate_body(&mut self) {
        // Generate crab shell (similar to torus but modified for crab shape)
        for i in 0..SHELL_SEGMENTS {
            let phi = 2.0 * PI * i as f64 / SHELL_SEGMENTS as f64;

            // Make it more crab-shaped: wider than tall
            let cx = f64::cos(phi) * R1 * 1.2; // Wider
            let cy = f64::sin(phi) * R1 * 0.9; // Slightly flatter

            for j in 0..SHELL_SEGMENTS {
                let theta = 2.0 * PI * j as f64 / SHELL_SEGMENTS as f64;

                // Modify this to make shell more like a crab carapace
                let r2_mod = if phi > PI * 0.5 && phi < PI * 1.5 {
                    // The back side is more rounded
                    R2 * (0.7 + 0.3 * f64::cos(theta))
                } else {
                    // The front is more pointed
                    R2 * (0.6 + 0.4 * f64::cos(theta)) * (1.0 + 0.2 * f64::cos(phi))
                };

                let nx = f64::cos(phi) * f64::cos(theta);
                let ny = f64::sin(phi) * f64::cos(theta);
                let nz = f64::sin(theta);

                let x = cx + nx * r2_mod;
                let y = cy + ny * r2_mod;
                let z = nz * r2_mod;

                self.body_points.push(Point3D::new(x, y, z));
                self.body_normals.push(Point3D::new(nx, ny, nz).normalize());
            }
        }
    }

    fn generate_legs(&mut self) {
        // Generate 4 pairs of legs
        for leg_pair in 0..4 {
            let angle_offset = (leg_pair as f64 - 1.5) * PI / 10.0;

            // Left leg
            let mut left_leg_points = Vec::new();
            let mut left_leg_normals = Vec::new();
            self.generate_leg(&mut left_leg_points, &mut left_leg_normals, -1.0, angle_offset);
            self.leg_points.push(left_leg_points);
            self.leg_normals.push(left_leg_normals);

            // Right leg
            let mut right_leg_points = Vec::new();
            let mut right_leg_normals = Vec::new();
            self.generate_leg(&mut right_leg_points, &mut right_leg_normals, 1.0, angle_offset);
            self.leg_points.push(right_leg_points);
            self.leg_normals.push(right_leg_normals);
        }
    }

    fn generate_leg(&self, points: &mut Vec<Point3D>, normals: &mut Vec<Point3D>, side: f64, angle_offset: f64) {
        let base_angle = if side < 0.0 { PI } else { 0.0 } + angle_offset;
        let leg_length = 16.0;
        let segments = LEG_SEGMENTS;

        // Start from body edge
        let start_x = f64::cos(base_angle) * R1 * 1.1;
        let start_y = f64::sin(base_angle) * R1 * 0.9;
        let start_z = -1.0;

        // Create a curved leg path
        for i in 0..=segments {
            let t = i as f64 / segments as f64;

            // Control points for a Bezier-like curve
            let cp1_x = start_x;
            let cp1_y = start_y;
            let cp1_z = start_z;

            let cp2_x = start_x + side * leg_length * 0.8;
            let cp2_y = start_y + leg_length * 0.1;
            let cp2_z = -3.0;

            let cp3_x = start_x + side * leg_length;
            let cp3_y = start_y + leg_length * 0.2;
            let cp3_z = -5.0 * (1.0 - t) + 2.0 * t; // Goes down then up at tip

            // Quadratic Bezier formula
            let ax = (1.0 - t) * cp1_x + t * cp2_x;
            let ay = (1.0 - t) * cp1_y + t * cp2_y;
            let az = (1.0 - t) * cp1_z + t * cp2_z;

            let bx = (1.0 - t) * cp2_x + t * cp3_x;
            let by = (1.0 - t) * cp2_y + t * cp3_y;
            let bz = (1.0 - t) * cp2_z + t * cp3_z;

            let x = (1.0 - t) * ax + t * bx;
            let y = (1.0 - t) * ay + t * by;
            let z = (1.0 - t) * az + t * bz;

            points.push(Point3D::new(x, y, z));

            // Approximate normal (not perfect but works for shading)
            let dx = if i < segments { points[i+1].x - points[i].x } else { points[i].x - points[i-1].x };
            let dy = if i < segments { points[i+1].y - points[i].y } else { points[i].y - points[i-1].y };
            let dz = if i < segments { points[i+1].z - points[i].z } else { points[i].z - points[i-1].z };

            // Create a normal perpendicular to the leg direction
            let leg_dir = Point3D::new(dx, dy, dz).normalize();
            let up = Point3D::new(0.0, 0.0, 1.0);
            let normal = Point3D::new(
                leg_dir.y * up.z - leg_dir.z * up.y,
                leg_dir.z * up.x - leg_dir.x * up.z,
                leg_dir.x * up.y - leg_dir.y * up.x
            ).normalize();

            normals.push(normal);
        }
    }

    fn generate_claws(&mut self) {
        // Generate left claw
        let mut left_claw_points = Vec::new();
        let mut left_claw_normals = Vec::new();
        self.generate_claw(&mut left_claw_points, &mut left_claw_normals, -1.0);
        self.claw_points.push(left_claw_points);
        self.claw_normals.push(left_claw_normals);

        // Generate right claw
        let mut right_claw_points = Vec::new();
        let mut right_claw_normals = Vec::new();
        self.generate_claw(&mut right_claw_points, &mut right_claw_normals, 1.0);
        self.claw_points.push(right_claw_points);
        self.claw_normals.push(right_claw_normals);
    }

    fn generate_claw(&self, points: &mut Vec<Point3D>, normals: &mut Vec<Point3D>, side: f64) {
        let base_angle = if side < 0.0 { PI * 0.8 } else { PI * 0.2 };
        let claw_length = 20.0;
        let segments = CLAW_SEGMENTS;

        // Start from body edge
        let start_x = f64::cos(base_angle) * R1 * 1.2;
        let start_y = f64::sin(base_angle) * R1 * 0.9;
        let start_z = 0.0;

        // Create the arm part of the claw
        for i in 0..=segments {
            let t = i as f64 / segments as f64;

            // Control points for a curved path
            let cp1_x = start_x;
            let cp1_y = start_y;
            let cp1_z = start_z;

            let cp2_x = start_x + side * claw_length * 0.7;
            let cp2_y = start_y - claw_length * 0.1;
            let cp2_z = 2.0;

            let cp3_x = start_x + side * claw_length;
            let cp3_y = start_y - claw_length * 0.2;
            let cp3_z = 4.0;

            // Quadratic Bezier formula
            let ax = (1.0 - t) * cp1_x + t * cp2_x;
            let ay = (1.0 - t) * cp1_y + t * cp2_y;
            let az = (1.0 - t) * cp1_z + t * cp2_z;

            let bx = (1.0 - t) * cp2_x + t * cp3_x;
            let by = (1.0 - t) * cp2_y + t * cp3_y;
            let bz = (1.0 - t) * cp2_z + t * cp3_z;

            let x = (1.0 - t) * ax + t * bx;
            let y = (1.0 - t) * ay + t * by;
            let z = (1.0 - t) * az + t * bz;

            points.push(Point3D::new(x, y, z));

            // Generate pincers at the end
            if i == segments {
                // Upper pincer
                for j in 0..=4 {
                    let pt = j as f64 / 4.0;
                    let px = x + side * 2.0 * pt;
                    let py = y - 1.0 * pt;
                    let pz = z + 1.0 - 2.0 * pt;
                    points.push(Point3D::new(px, py, pz));
                }

                // Lower pincer
                for j in 0..=4 {
                    let pt = j as f64 / 4.0;
                    let px = x + side * 2.0 * pt;
                    let py = y + 1.0 * pt;
                    let pz = z - 1.0 + 0.0 * pt;
                    points.push(Point3D::new(px, py, pz));
                }
            }

            // Approximate normal
            let normal = Point3D::new(0.0, side, 0.0).normalize();
            normals.push(normal);

            // Add normals for pincers
            if i == segments {
                for _ in 0..=4 {
                    normals.push(Point3D::new(0.0, side, 0.2).normalize());
                }
                for _ in 0..=4 {
                    normals.push(Point3D::new(0.0, side, -0.2).normalize());
                }
            }
        }
    }

    fn generate_eyes(&mut self) {
        // Two eyes on top of the crab
        let eye_distance = 4.0;

        for side in [-1.0, 1.0] {
            let eye_x = side * eye_distance;
            let eye_y = R1 * 0.5;
            let eye_z = R2 * 0.8;

            // Create a spherical eye
            for phi in 0..8 {
                let phi_rad = PI * phi as f64 / 8.0;
                for theta in 0..8 {
                    let theta_rad = 2.0 * PI * theta as f64 / 8.0;

                    let eye_size = 1.5;
                    let nx = f64::sin(phi_rad) * f64::cos(theta_rad);
                    let ny = f64::sin(phi_rad) * f64::sin(theta_rad);
                    let nz = f64::cos(phi_rad);

                    let x = eye_x + nx * eye_size;
                    let y = eye_y + ny * eye_size;
                    let z = eye_z + nz * eye_size;

                    self.eye_points.push(Point3D::new(x, y, z));
                    self.eye_normals.push(Point3D::new(nx, ny, nz).normalize());
                }
            }
        }
    }

    // Added render function to fix missing implementation
    fn render(&self, frame: f64) -> String {
        let buffer_size = WIDTH * HEIGHT;
        let mut output_buffer = vec![' '; buffer_size];
        let mut z_buffer = vec![0.0; buffer_size];

        self.animate(frame, &mut z_buffer, &mut output_buffer);

        // Convert the buffer to a string
        let mut output = String::new();
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let idx = y * WIDTH + x;
                output.push(output_buffer[idx]);
            }
            output.push('\n');
        }

        output
    }

    fn animate(&self, frame: f64, z_buffer: &mut [f64], output_buffer: &mut [char]) {
        // Clear buffers
        z_buffer.fill(0.0);
        output_buffer.fill(' ');

        // Create an animated light direction that moves slightly
        let light_x = LIGHT_DIRECTION[0];
        let light_y = LIGHT_DIRECTION[1] * f64::cos(frame * 0.1) - LIGHT_DIRECTION[2] * f64::sin(frame * 0.1);
        let light_z = LIGHT_DIRECTION[1] * f64::sin(frame * 0.1) + LIGHT_DIRECTION[2] * f64::cos(frame * 0.1);
        let light_dir = Point3D::new(light_x, light_y, light_z).normalize();

        // Dance animation parameters
        let bounce = f64::sin(frame * 0.8) * 2.0;
        let spin = frame * 0.3;
        let claw_wave = f64::sin(frame * 1.2) * 0.15;
        let leg_wave = frame * 2.0;

        // Render body
        for (i, point) in self.body_points.iter().enumerate() {
            let mut p = *point;

            // Apply rotations
            p.rotate_y(spin);
            p.rotate_x(f64::sin(frame * 0.2) * 0.1);
            p.rotate_z(f64::cos(frame * 0.3) * 0.05);

            // Apply bounce
            p.y += bounce;

            let (x2d, y2d, z) = p.project();
            if z < 0.0 || x2d < 0.0 || x2d >= WIDTH as f64 || y2d < 0.0 || y2d >= HEIGHT as f64 {
                continue;
            }

            let x = x2d as usize;
            let y = y2d as usize;
            
            // Additional safety check
            if x >= WIDTH || y >= HEIGHT {
                continue;
            }
            
            let idx = y * WIDTH + x;

            if idx >= z_buffer.len() {
                continue;
            }

            if z_buffer[idx] == 0.0 || z < z_buffer[idx] {
                // Calculate luminance based on normal and light direction
                let mut normal = self.body_normals[i];
                normal.rotate_y(spin);
                normal.rotate_x(f64::sin(frame * 0.2) * 0.1);
                normal.rotate_z(f64::cos(frame * 0.3) * 0.05);

                let luminance = normal.dot(&light_dir);

                if luminance > 0.0 {
                    z_buffer[idx] = z;
                    let lum_idx = (luminance * (SHADE_CHARS.len() - 1) as f64).round() as usize;
                    let shade_char = SHADE_CHARS.chars().nth(lum_idx).unwrap_or('@');
                    output_buffer[idx] = shade_char;
                }
            }
        }

        // Render eyes (always on top for visibility)
        for (_i, point) in self.eye_points.iter().enumerate() {
            let mut p = *point;

            // Apply rotations
            p.rotate_y(spin);
            p.rotate_x(f64::sin(frame * 0.2) * 0.1);
            p.rotate_z(f64::cos(frame * 0.3) * 0.05);

            // Apply bounce
            p.y += bounce;

            let (x2d, y2d, z) = p.project();
            if z < 0.0 || x2d < 0.0 || x2d >= WIDTH as f64 || y2d < 0.0 || y2d >= HEIGHT as f64 {
                continue;
            }

            let x = x2d as usize;
            let y = y2d as usize;
            
            // Additional safety check
            if x >= WIDTH || y >= HEIGHT {
                continue;
            }
            
            let idx = y * WIDTH + x;

            if idx >= z_buffer.len() {
                continue;
            }

            if z_buffer[idx] == 0.0 || z < z_buffer[idx] {
                // Eyes always use O or @ for visibility
                z_buffer[idx] = z;
                output_buffer[idx] = '@';
            }
        }

        // Render legs with wave animation
        for (leg_idx, leg_points) in self.leg_points.iter().enumerate() {
            let leg_phase = leg_wave + PI * (leg_idx % 4) as f64 / 2.0;
            let leg_wave_intensity = if leg_idx % 2 == 0 { 1.0 } else { -1.0 } * 0.2;

            for (i, point) in leg_points.iter().enumerate() {
                let mut p = *point;

                // Apply leg-specific animation
                let bend_factor = (i as f64 / leg_points.len() as f64) * f64::sin(leg_phase) * leg_wave_intensity;
                p.z += bend_factor * 3.0;

                // Apply global rotations
                p.rotate_y(spin);
                p.rotate_x(f64::sin(frame * 0.2) * 0.1);
                p.rotate_z(f64::cos(frame * 0.3) * 0.05);

                // Apply bounce
                p.y += bounce;

                let (x2d, y2d, z) = p.project();
                if z < 0.0 || x2d < 0.0 || x2d >= WIDTH as f64 || y2d < 0.0 || y2d >= HEIGHT as f64 {
                    continue;
                }

                let x = x2d as usize;
                let y = y2d as usize;
                
                // Additional safety check
                if x >= WIDTH || y >= HEIGHT {
                    continue;
                }
                
                let idx = y * WIDTH + x;

                if idx >= z_buffer.len() {
                    continue;
                }

                if z_buffer[idx] == 0.0 || z < z_buffer[idx] {
                    let mut normal = self.leg_normals[leg_idx][i];
                    normal.rotate_y(spin);
                    normal.rotate_x(f64::sin(frame * 0.2) * 0.1);
                    normal.rotate_z(f64::cos(frame * 0.3) * 0.05);

                    let luminance = normal.dot(&light_dir);

                    if luminance > 0.0 {
                        z_buffer[idx] = z;
                        let lum_idx = (luminance * (SHADE_CHARS.len() - 1) as f64).round() as usize;
                        let shade_char = SHADE_CHARS.chars().nth(lum_idx).unwrap_or('#');
                        output_buffer[idx] = shade_char;
                    }
                }
            }
        }

        // Render claws with wave animation
        for (claw_idx, claw_points) in self.claw_points.iter().enumerate() {
            let claw_dir = if claw_idx == 0 { -1.0 } else { 1.0 };

            for (i, point) in claw_points.iter().enumerate() {
                let mut p = *point;

                // Apply claw-specific animation (waves)
                p.rotate_z(claw_wave * claw_dir * (i as f64 / claw_points.len() as f64));

                // Apply global rotations
                p.rotate_y(spin);
                p.rotate_x(f64::sin(frame * 0.2) * 0.1);
                p.rotate_z(f64::cos(frame * 0.3) * 0.05);

                // Apply bounce
                p.y += bounce;

                let (x2d, y2d, z) = p.project();
                if z < 0.0 || x2d < 0.0 || x2d >= WIDTH as f64 || y2d < 0.0 || y2d >= HEIGHT as f64 {
                    continue;
                }

                let x = x2d as usize;
                let y = y2d as usize;
                
                // Additional safety check
                if x >= WIDTH || y >= HEIGHT {
                    continue;
                }
                
                let idx = y * WIDTH + x;

                if idx >= z_buffer.len() {
                    continue;
                }

                if z_buffer[idx] == 0.0 || z < z_buffer[idx] {
                    let mut normal = self.claw_normals[claw_idx][i];
                    normal.rotate_z(claw_wave * claw_dir * (i as f64 / claw_points.len() as f64));
                    normal.rotate_y(spin);
                    normal.rotate_x(f64::sin(frame * 0.2) * 0.1);
                    normal.rotate_z(f64::cos(frame * 0.3) * 0.05);

                    let luminance = normal.dot(&light_dir);

                    if luminance > 0.0 {
                        z_buffer[idx] = z;
                        let lum_idx = (luminance * (SHADE_CHARS.len() - 1) as f64).round() as usize;
                        let shade_char = SHADE_CHARS.chars().nth(lum_idx).unwrap_or('$');
                        output_buffer[idx] = shade_char;
                    }
                }
            }
        }

        // Add detailed crab pattern using Rust logo inspiration
        self.add_crab_details(frame, spin, bounce, z_buffer, output_buffer);
    }

    fn add_crab_details(&self, frame: f64, spin: f64, bounce: f64, z_buffer: &mut [f64], output_buffer: &mut [char]) {
        // Enhanced crab shell texture patterns
        let detail_count = 12;
        for i in 0..detail_count {
            let angle = 2.0 * PI * i as f64 / detail_count as f64;

            // Main shell pattern
            let pattern_x = f64::cos(angle) * R1 * 0.7;
            let pattern_y = f64::sin(angle) * R1 * 0.6;
            let pattern_z = R2 * 1.1;

            let mut pattern_point = Point3D::new(pattern_x, pattern_y, pattern_z);
            pattern_point.rotate_y(spin);
            pattern_point.rotate_x(f64::sin(frame * 0.2) * 0.1);
            pattern_point.rotate_z(f64::cos(frame * 0.3) * 0.05);
            pattern_point.y += bounce;

            let (x2d, y2d, z) = pattern_point.project();
            if z > 0.0 && x2d >= 0.0 && x2d < WIDTH as f64 && y2d >= 0.0 && y2d < HEIGHT as f64 {
                let x = x2d as usize;
                let y = y2d as usize;
                let idx = y * WIDTH + x;

                if idx < z_buffer.len() && (z_buffer[idx] == 0.0 || z < z_buffer[idx]) {
                    z_buffer[idx] = z;
                    output_buffer[idx] = '%';
                }
            }

            // Add smaller details
            let small_angle = angle + PI / detail_count as f64;
            let small_x = f64::cos(small_angle) * R1 * 0.5;
            let small_y = f64::sin(small_angle) * R1 * 0.4;
            let small_z = R2 * 1.05;

            let mut small_point = Point3D::new(small_x, small_y, small_z);
            small_point.rotate_y(spin);
            small_point.rotate_x(f64::sin(frame * 0.2) * 0.1);
            small_point.rotate_z(f64::cos(frame * 0.3) * 0.05);
            small_point.y += bounce;

            let (sx2d, sy2d, sz) = small_point.project();
            if sz > 0.0 && sx2d >= 0.0 && sx2d < WIDTH as f64 && sy2d >= 0.0 && sy2d < HEIGHT as f64 {
                let sx = sx2d as usize;
                let sy = sy2d as usize;
                let sidx = sy * WIDTH + sx;

                if sidx < z_buffer.len() && (z_buffer[sidx] == 0.0 || sz < z_buffer[sidx]) {
                    z_buffer[sidx] = sz;
                    output_buffer[sidx] = '.';
                }
            }
        }
    }
}

fn main() -> io::Result<()> {
    // Create the crab model
    let crab = CrabModel::new();

    // Set up terminal
    let mut stdout = io::stdout();
    let mut frame_count: f64 = 0.0;
    let frame_duration = Duration::from_millis(50);
    let frame_increment = 0.1;

    // Animation loop
    loop {
        let start_time = std::time::Instant::now();

        // Clear the screen (ANSI escape code)
        print!("\x1B[2J\x1B[1;1H");

        // Render the crab at current frame
        let output = crab.render(frame_count);

        // Print the frame
        stdout.write_all(output.as_bytes())?;
        stdout.flush()?;

        // Calculate elapsed time and sleep for consistent frame rate
        let elapsed = start_time.elapsed();
        if elapsed < frame_duration {
            thread::sleep(frame_duration - elapsed);
        }

        // Update frame counter
        frame_count += frame_increment;
    }
}
