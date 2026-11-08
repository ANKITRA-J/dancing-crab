mod display;
mod render;

use std::io::{self, IsTerminal, Write};
use std::time::{Duration, Instant};

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute, queue,
    style::{Color, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use display::{compose, Cell, Style};
use render::Renderer;

const HELP: &str = "Dancing Crab - a shaded 3D crab in your terminal

Usage: dancing-crab [OPTIONS]

  --style STYLE     ascii (classic symbols) or smooth (finer Unicode dots)
  --snapshot        Print one plain frame in the selected style and exit
  --no-color        Disable orange terminal colors (also respects NO_COLOR)
  --time SECONDS    Starting animation time (default: 0)
  --width COLUMNS   Canvas width, 20..400 (default: fit terminal, or 100)
  --height ROWS     Canvas height, 8..120 (default: fit terminal, or 32)
  --fps FPS         Frames per second, 1..120 (default: 60)
  --frames COUNT    Exit after this many frames
  -h, --help        Show this help

Controls: M switches styles; Space pauses; Q, Esc, or Ctrl+C quits.
When output or input is not a terminal, print one plain frame by default.
";

#[derive(Debug)]
struct Options {
    snapshot: bool,
    no_color: bool,
    style: Style,
    time: f64,
    width: Option<usize>,
    height: Option<usize>,
    fps: u32,
    frames: Option<usize>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            snapshot: false,
            no_color: false,
            style: Style::Ascii,
            time: 0.0,
            width: None,
            height: None,
            fps: 60,
            frames: None,
        }
    }
}

fn parse_options(args: impl IntoIterator<Item = String>) -> Result<Option<Options>, String> {
    let mut options = Options::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == "--help" || arg == "-h" {
            return Ok(None);
        }
        if arg == "--snapshot" {
            options.snapshot = true;
            continue;
        }
        if arg == "--no-color" {
            options.no_color = true;
            continue;
        }
        if !matches!(
            arg.as_str(),
            "--style" | "--time" | "--width" | "--height" | "--fps" | "--frames"
        ) {
            return Err(format!("Unknown option: {arg}. Use --help for usage."));
        }
        let value = args.next().ok_or_else(|| format!("{arg} needs a value"))?;
        match arg.as_str() {
            "--style" => options.style = Style::parse(&value)?,
            "--time" => {
                let time: f64 = value.parse().map_err(|_| "--time needs a number")?;
                if !time.is_finite() || time < 0.0 {
                    return Err("--time must be a finite, nonnegative number".into());
                }
                options.time = time;
            }
            "--width" => options.width = Some(parse_range(&arg, &value, 20, 400)?),
            "--height" => options.height = Some(parse_range(&arg, &value, 8, 120)?),
            "--fps" => options.fps = parse_range(&arg, &value, 1, 120)? as u32,
            "--frames" => options.frames = Some(parse_range(&arg, &value, 1, usize::MAX)?),
            _ => unreachable!(),
        }
    }
    Ok(Some(options))
}

fn parse_range(name: &str, value: &str, min: usize, max: usize) -> Result<usize, String> {
    value
        .parse::<usize>()
        .ok()
        .filter(|value| (min..=max).contains(value))
        .ok_or_else(|| format!("{name} must be a whole number from {min} to {max}"))
}

struct Terminal;

impl Terminal {
    fn enter(out: &mut impl Write) -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let guard = Self;
        execute!(out, EnterAlternateScreen, Hide, Clear(ClearType::All))?;
        Ok(guard)
    }
}

fn restore_terminal() {
    let _ = terminal::disable_raw_mode();
    let _ = execute!(io::stdout(), ResetColor, Show, LeaveAlternateScreen);
}

impl Drop for Terminal {
    fn drop(&mut self) {
        restore_terminal();
    }
}

fn write_line(out: &mut impl Write, row: u16, text: &str, columns: u16) -> io::Result<()> {
    queue!(out, MoveTo(0, row))?;
    // Leave the last column unused to avoid automatic wrapping and scrolling.
    let end = text.len().min(columns.saturating_sub(1) as usize);
    out.write_all(&text.as_bytes()[..end])?;
    queue!(out, Clear(ClearType::UntilNewLine))
}

fn write_crab_row(
    out: &mut impl Write,
    row: u16,
    cells: &[Cell],
    columns: u16,
    color: bool,
) -> io::Result<()> {
    queue!(out, MoveTo(0, row))?;
    let cells = &cells[..cells.len().min(columns.saturating_sub(1) as usize)];
    let mut foreground = None;
    let mut background = None;
    let mut utf8 = [0; 4];
    for cell in cells {
        if color && cell.color != foreground {
            let shade = match cell.color {
                Some((r, g, b)) => Color::Rgb { r, g, b },
                None => Color::Reset,
            };
            queue!(out, SetForegroundColor(shade))?;
            foreground = cell.color;
        }
        if color && cell.background != background {
            let shade = match cell.background {
                Some((r, g, b)) => Color::Rgb { r, g, b },
                None => Color::Reset,
            };
            queue!(out, SetBackgroundColor(shade))?;
            background = cell.background;
        }
        out.write_all(cell.glyph.encode_utf8(&mut utf8).as_bytes())?;
    }
    if color {
        queue!(out, ResetColor)?;
    }
    queue!(out, Clear(ClearType::UntilNewLine))
}

fn renderer_for(style: Style, width: usize, height: usize) -> Renderer {
    let (raster_width, raster_height, aspect) = style.raster_size(width, height);
    Renderer::with_aspect(raster_width, raster_height, aspect)
}

fn animate(options: &Options) -> io::Result<()> {
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        previous_hook(info);
    }));
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let _terminal = Terminal::enter(&mut out)?;
    let tick = Duration::from_secs_f64(1.0 / f64::from(options.fps));
    let color = !options.no_color && std::env::var_os("NO_COLOR").is_none();
    let mut time = options.time;
    let mut last_tick = Instant::now();
    let mut paused = false;
    let mut rendered = 0;
    let mut style = options.style;
    let mut canvas = None;
    let mut renderer = Renderer::new(20, 8);
    loop {
        let started = Instant::now();
        if !paused {
            time += started.duration_since(last_tick).as_secs_f64();
        }
        last_tick = started;
        let (columns, rows) = terminal::size()?;
        let width = options
            .width
            .unwrap_or(400)
            .min(columns.saturating_sub(1) as usize);
        let height = options
            .height
            .unwrap_or(120)
            .min(rows.saturating_sub(2) as usize);
        if canvas != Some((width, height, style)) {
            execute!(out, Clear(ClearType::All))?;
            renderer = renderer_for(style, width, height);
            canvas = Some((width, height, style));
        }
        if width < 20 || height < 8 {
            write_line(&mut out, 0, "Resize terminal to at least 21 x 10", columns)?;
        } else {
            write_line(&mut out, 0, "D A N C I N G   C R A B", columns)?;
            let frame = compose(style, &renderer.render(time), width, height);
            for (y, cells) in frame.rows().enumerate() {
                write_crab_row(&mut out, y as u16 + 1, cells, columns, color)?;
            }
            let playback = if paused {
                format!("{} (PAUSED)", style.label())
            } else {
                style.label().to_owned()
            };
            let footer = format!("{playback} | M: style | Space: pause | Q / Esc / Ctrl+C: quit");
            write_line(&mut out, height as u16 + 1, &footer, columns)?;
        }
        out.flush()?;
        rendered += 1;
        if options.frames.is_some_and(|limit| rendered >= limit) {
            break;
        }
        // Read input while waiting for the next frame, so controls stay responsive.
        let deadline = started + tick;
        loop {
            let wait = deadline.saturating_duration_since(Instant::now());
            if !event::poll(wait)? {
                break;
            }
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q' | 'Q') | KeyCode::Esc => return Ok(()),
                        KeyCode::Char('c' | 'C')
                            if key.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            return Ok(());
                        }
                        KeyCode::Char(' ') => paused = !paused,
                        KeyCode::Char('m' | 'M') => style.toggle(),
                        _ => {}
                    }
                }
            }
            if Instant::now() >= deadline {
                break;
            }
        }
    }
    Ok(())
}

fn print_frames(options: &Options) -> io::Result<()> {
    let width = options.width.unwrap_or(100);
    let height = options.height.unwrap_or(32);
    let mut renderer = renderer_for(options.style, width, height);
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let count = if options.snapshot {
        1
    } else {
        options.frames.unwrap_or(1)
    };
    for frame in 0..count {
        if frame > 0 {
            writeln!(out)?;
        }
        let time = options.time + frame as f64 / f64::from(options.fps);
        let frame = compose(options.style, &renderer.render(time), width, height);
        writeln!(out, "{}", frame.plain_text())?;
    }
    out.flush()
}

fn run() -> io::Result<()> {
    let options = match parse_options(std::env::args().skip(1)) {
        Ok(Some(options)) => options,
        Ok(None) => {
            print!("{HELP}");
            return Ok(());
        }
        Err(error) => {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, error));
        }
    };
    if options.snapshot || !io::stdout().is_terminal() || !io::stdin().is_terminal() {
        print_frames(&options)
    } else {
        animate(&options)
    }
}

fn main() {
    if let Err(error) = run() {
        if error.kind() != io::ErrorKind::BrokenPipe {
            eprintln!("dancing-crab: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Option<Options>, String> {
        parse_options(args.iter().map(|arg| (*arg).to_owned()))
    }

    #[test]
    fn rejects_invalid_values_before_entering_raw_mode() {
        for args in [
            vec!["--fps", "0"],
            vec!["--width", "1000000000"],
            vec!["--height", "-1"],
            vec!["--time", "NaN"],
            vec!["--time", "inf"],
            vec!["--frames", "0"],
            vec!["--width"],
            vec!["--unknown"],
            vec!["--style", "invalid"],
            vec!["--style"],
        ] {
            assert!(parse(&args).is_err(), "should reject {args:?}");
        }
    }
}
