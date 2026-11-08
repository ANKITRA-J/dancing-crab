//! Convert a sampled surface into either classic symbols or finer terminal dots.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Style {
    #[default]
    Ascii,
    Smooth,
}

impl Style {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "ascii" => Ok(Self::Ascii),
            "smooth" | "braille" => Ok(Self::Smooth),
            _ => Err("--style must be ascii or smooth".into()),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Ascii => "ASCII",
            Self::Smooth => "SMOOTH",
        }
    }

    pub fn toggle(&mut self) {
        *self = match self {
            Self::Ascii => Self::Smooth,
            Self::Smooth => Self::Ascii,
        };
    }

    /// Width, height, and vertical scale for the model's sampling grid.
    pub fn raster_size(self, columns: usize, rows: usize) -> (usize, usize, f64) {
        match self {
            Self::Ascii => (columns, rows, 0.5),
            Self::Smooth => (columns * 2, rows * 4, 1.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub glyph: char,
    pub color: Option<(u8, u8, u8)>,
    pub background: Option<(u8, u8, u8)>,
}

#[derive(Clone, Debug)]
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub cells: Vec<Cell>,
}

impl Frame {
    /// Terminal rows, measured in cells rather than UTF-8 bytes.
    pub fn rows(&self) -> impl Iterator<Item = &[Cell]> {
        self.cells.chunks(self.width.max(1)).take(self.height)
    }

    pub fn plain_text(&self) -> String {
        let mut text = String::with_capacity(self.cells.len() * 3 + self.height);
        for (index, row) in self.rows().enumerate() {
            if index != 0 {
                text.push('\n');
            }
            for cell in row {
                text.push(cell.glyph);
            }
        }
        text
    }
}

fn color_for(sample: u8) -> Option<(u8, u8, u8)> {
    match sample {
        b'O' | b'_' => Some((255, 244, 225)),
        b'o' => Some((90, 52, 37)),
        b'.' | b',' | b'-' | b'~' => Some((166, 57, 9)),
        b':' | b';' | b'=' => Some((215, 68, 8)),
        b'!' | b'*' | b'#' => Some((247, 76, 0)),
        b'$' | b'@' => Some((255, 151, 64)),
        _ => None,
    }
}

fn smooth_shell_color(sample: u8) -> Option<(u8, u8, u8)> {
    const SHADES: &[u8] = b".,-~:;=!*#$@";
    let index = SHADES.iter().position(|shade| *shade == sample)? as i32;
    let (start, end, position, steps) = if index <= 6 {
        ((115, 38, 8), (247, 76, 0), index, 6)
    } else {
        ((247, 76, 0), (255, 182, 88), index - 6, 5)
    };
    let interpolate = |a: i32, b: i32| (a + (b - a) * position / steps) as u8;
    Some((
        interpolate(start.0, end.0),
        interpolate(start.1, end.1),
        interpolate(start.2, end.2),
    ))
}

/// Compose exactly `columns * rows` terminal cells from an ASCII surface raster.
/// Smooth mode uses the eight dot positions in each Braille character, preserving
/// small edge changes that would occupy only a fraction of an ASCII cell.
pub fn compose(style: Style, raster: &str, columns: usize, rows: usize) -> Frame {
    let source: Vec<_> = raster.lines().map(str::as_bytes).collect();
    let sample = |x: usize, y: usize| {
        source
            .get(y)
            .and_then(|line| line.get(x))
            .copied()
            .unwrap_or(b' ')
    };
    let mut cells = Vec::with_capacity(columns * rows);
    for y in 0..rows {
        for x in 0..columns {
            let cell = match style {
                Style::Ascii => {
                    let value = sample(x, y);
                    Cell {
                        glyph: char::from(value),
                        color: color_for(value),
                        background: None,
                    }
                }
                Style::Smooth => smooth_cell(x, y, &sample),
            };
            cells.push(cell);
        }
    }
    Frame {
        width: columns,
        height: rows,
        cells,
    }
}

fn smooth_cell(x: usize, y: usize, sample: &impl Fn(usize, usize) -> u8) -> Cell {
    // Unicode's Braille dots run down each column; the bottom pair is dots 7/8.
    const DOTS: [[u32; 2]; 4] = [[0, 3], [1, 4], [2, 5], [6, 7]];
    let mut mask = 0;
    let mut eyes = 0;
    let mut count = 0;
    let mut shell_count = 0;
    let mut channels = [0_u32; 3];
    for (dy, dots) in DOTS.iter().enumerate() {
        for (dx, dot) in dots.iter().enumerate() {
            let value = sample(x * 2 + dx, y * 4 + dy);
            // Pupils are small holes in the eye surface. They remain visible when
            // color is disabled, instead of merging into a solid eye silhouette.
            if value == b'o' {
                continue;
            }
            let eye = matches!(value, b'O' | b'_');
            let color = if eye {
                Some((255, 244, 225))
            } else {
                smooth_shell_color(value)
            };
            let Some((r, g, b)) = color else { continue };
            mask |= 1 << dot;
            if !eye {
                channels[0] += u32::from(r);
                channels[1] += u32::from(g);
                channels[2] += u32::from(b);
                shell_count += 1;
            }
            count += 1;
            eyes += u32::from(eye);
        }
    }
    if mask == 0 {
        return Cell {
            glyph: ' ',
            color: None,
            background: None,
        };
    }
    let color = if eyes * 2 >= count {
        (255, 244, 225)
    } else {
        (
            (channels[0] / shell_count) as u8,
            (channels[1] / shell_count) as u8,
            (channels[2] / shell_count) as u8,
        )
    };
    Cell {
        glyph: char::from_u32(0x2800 + mask).unwrap(),
        color: Some(color),
        // Fill complete cells of one material. Boundaries and pupil holes keep
        // individual dots, preserving the finer silhouette and eye detail.
        background: (mask == 0xff && (shell_count == 8 || eyes == 8)).then_some(color),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_subpixel_uses_its_correct_braille_dot() {
        let positions = [
            (0, 0, 0x01),
            (0, 1, 0x02),
            (0, 2, 0x04),
            (1, 0, 0x08),
            (1, 1, 0x10),
            (1, 2, 0x20),
            (0, 3, 0x40),
            (1, 3, 0x80),
        ];
        for (x, y, mask) in positions {
            let mut raster = [b' '; 11];
            raster[2] = b'\n';
            raster[5] = b'\n';
            raster[8] = b'\n';
            raster[y * 3 + x] = b'#';
            let frame = compose(Style::Smooth, std::str::from_utf8(&raster).unwrap(), 1, 1);
            assert_eq!(frame.cells[0].glyph as u32, 0x2800 + mask);
            assert_eq!(frame.cells[0].color, smooth_shell_color(b'#'));
        }
    }

    #[test]
    fn empty_surface_is_spaces_and_output_dimensions_are_cells() {
        let frame = compose(Style::Smooth, "", 5, 3);
        assert_eq!((frame.width, frame.height), (5, 3));
        assert_eq!(frame.rows().count(), 3);
        assert_eq!(frame.plain_text(), "     \n     \n     ");
        assert!(frame.cells.iter().all(|cell| cell.color.is_none()));

        let frame = compose(Style::Smooth, "##\n##\n##\n##", 1, 1);
        assert_eq!(frame.plain_text(), "⣿");
        assert_eq!(frame.plain_text().chars().count(), 1);
        assert_eq!(frame.plain_text().len(), 3);
    }

    #[test]
    fn ascii_preserves_symbols_and_eye_colors() {
        let frame = compose(Style::Ascii, " O#\n_o ", 3, 2);
        assert_eq!(frame.plain_text(), " O#\n_o ");
        assert_eq!(frame.cells[1].color, Some((255, 244, 225)));
        assert_eq!(frame.cells[2].color, Some((247, 76, 0)));
        assert_eq!(frame.cells[4].color, Some((90, 52, 37)));
        assert!(frame.cells[0].color.is_none());
    }

    #[test]
    fn pupils_remain_holes_without_color() {
        let frame = compose(Style::Smooth, "OO\nOo\nOO\nOO", 1, 1);
        assert_eq!(frame.cells[0].glyph as u32, 0x2800 + 0xef);
        assert_eq!(frame.cells[0].color, Some((255, 244, 225)));
        assert_eq!(
            compose(Style::Smooth, "oo\noo\noo\noo", 1, 1).plain_text(),
            " "
        );
    }

    #[test]
    fn subpixel_edges_change_without_changing_terminal_dimensions() {
        let left = compose(Style::Smooth, "# \n# \n# \n# ", 1, 1);
        let right = compose(Style::Smooth, " #\n #\n #\n #", 1, 1);
        assert_ne!(left.plain_text(), right.plain_text());
        assert_eq!(left.cells.len(), right.cells.len());
        assert_eq!(Style::Ascii.raster_size(80, 24), (80, 24, 0.5));
        assert_eq!(Style::Smooth.raster_size(80, 24), (160, 96, 1.0));
    }

    #[test]
    fn smooth_lighting_keeps_more_shades_than_the_ascii_palette() {
        let mid = compose(Style::Smooth, ";;\n;;\n;;\n;;", 1, 1);
        let bright = compose(Style::Smooth, "@@\n@@\n@@\n@@", 1, 1);
        let mixed = compose(Style::Smooth, ";@\n;@\n;@\n;@", 1, 1);
        assert_eq!(bright.cells[0].color, Some((255, 182, 88)));
        let low = mid.cells[0].color.unwrap();
        let high = bright.cells[0].color.unwrap();
        let average = mixed.cells[0].color.unwrap();
        assert!(low.1 < average.1 && average.1 < high.1);
        assert_eq!(mid.cells[0].glyph, bright.cells[0].glyph);
        assert_eq!(mixed.cells[0].glyph, '⣿');
    }

    #[test]
    fn smooth_background_fills_only_complete_unmixed_cells() {
        let shell = compose(Style::Smooth, "##\n##\n##\n##", 1, 1);
        assert_eq!(shell.cells[0].background, shell.cells[0].color);
        assert!(shell.cells[0].background.is_some());
        let eye = compose(Style::Smooth, "OO\nOO\nOO\nOO", 1, 1);
        assert_eq!(eye.cells[0].background, Some((255, 244, 225)));

        for raster in [
            " #\n##\n##\n##",
            "OO\nOo\nOO\nOO",
            "O#\n##\n##\n##",
            "  \n  \n  \n  ",
        ] {
            assert!(compose(Style::Smooth, raster, 1, 1).cells[0]
                .background
                .is_none());
        }
        assert!(compose(Style::Ascii, "#", 1, 1).cells[0]
            .background
            .is_none());
    }
}
