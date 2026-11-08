//! yavcpdf: a minimal, keyboard-driven PDF viewer on the PdfCraft render engine.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod actions;
mod app;
mod cache;
mod chrome;
mod complete;
mod config;
mod contents;
mod doc;
mod draw;
mod export;
mod field;
mod hint;
mod input;
mod keymap;
mod overview;
mod search;
mod smart;
mod store;
mod view;

use std::path::PathBuf;
use std::process::ExitCode;

use config::Config;
use doc::Doc;

const USAGE: &str = "usage: yavcpdf [--page N] [--password PW] [--geometry WxH[+X+Y]]
             [--fullscreen|--no-fullscreen] [--statusbar|--no-statusbar] [file.pdf]

With no file, the document opened last is opened again.
--geometry sets the window size in points, and optionally its position.
The configuration is ~/.config/yavcpdf/config.toml.";

#[derive(Debug, Default, PartialEq)]
struct Args {
    file: Option<PathBuf>,
    page: Option<usize>,
    password: Option<String>,
    geometry: Option<Geometry>,
    fullscreen: Option<bool>,
    statusbar: Option<bool>,
    help: bool,
}

/// Largest window width, height or offset accepted by `--geometry`, in points.
const MAX_WINDOW_SIDE: u32 = 16384;

/// A window size, and optionally the position of its outer top-left corner.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Geometry {
    size: (u32, u32),
    position: Option<(u32, u32)>,
}

/// Parses `WxH` or `WxH+X+Y`: decimal digits only, W and H from 1 to 16384,
/// X and Y from 0 to 16384.
/// X11's negative offsets (from the right or bottom edge) are not supported.
fn parse_geometry(value: &str) -> Result<Geometry, String> {
    let bad = |why: &str| format!("--geometry: \"{value}\" {why}; expected WxH or WxH+X+Y, like 1000x700+40+30");
    if value.contains('-') {
        return Err(bad("has a negative number (offsets from the right or bottom edge are not supported)"));
    }
    let number = |s: &str| -> Option<u32> {
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        s.parse().ok()
    };
    let mut offsets = value.split('+');
    let size = offsets.next().unwrap_or_default();
    let (w, h) = size.split_once('x').ok_or_else(|| bad("is not a size"))?;
    let (w, h) = (number(w).ok_or_else(|| bad("has a bad width"))?, number(h).ok_or_else(|| bad("has a bad height"))?);
    if w == 0 || h == 0 {
        return Err(bad("has a width or height of zero"));
    }
    if w > MAX_WINDOW_SIDE || h > MAX_WINDOW_SIDE {
        return Err(bad(&format!("is larger than {MAX_WINDOW_SIDE}")));
    }
    let position = match (offsets.next(), offsets.next(), offsets.next()) {
        (None, ..) => None,
        (Some(x), Some(y), None) => {
            let (x, y) = (number(x).ok_or_else(|| bad("has a bad X offset"))?, number(y).ok_or_else(|| bad("has a bad Y offset"))?);
            // X11 window coordinates are 16 bits on the wire, so larger offsets can wrap off screen.
            if x > MAX_WINDOW_SIDE || y > MAX_WINDOW_SIDE {
                return Err(bad(&format!("has an offset larger than {MAX_WINDOW_SIDE}")));
            }
            Some((x, y))
        }
        (Some(_), None, _) => return Err(bad("has only one of X and Y")),
        _ => return Err(bad("has trailing text")),
    };
    Ok(Geometry { size: (w, h), position })
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut out = Args::default();
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => out.help = true,
            "--page" => {
                let value = args.next().ok_or("--page needs a number")?;
                let page: usize = value.parse().map_err(|_| format!("--page: \"{value}\" is not a number"))?;
                out.page = Some(page.saturating_sub(1));
            }
            "--geometry" => out.geometry = Some(parse_geometry(&args.next().ok_or("--geometry needs a value like 1000x700")?)?),
            "--fullscreen" => out.fullscreen = Some(true),
            "--no-fullscreen" => out.fullscreen = Some(false),
            "--statusbar" => out.statusbar = Some(true),
            "--no-statusbar" => out.statusbar = Some(false),
            "--password" => out.password = Some(args.next().ok_or("--password needs a value")?),
            flag if flag.starts_with('-') && flag.len() > 1 => return Err(format!("unknown option {flag}")),
            file if out.file.is_none() => out.file = Some(PathBuf::from(file)),
            _ => return Err("only one file can be opened".to_owned()),
        }
    }
    Ok(out)
}

fn run(args: Args) -> Result<(), String> {
    // First, as the last document and the config are both read from these.
    let adoption = store::adopt_old_folders();
    let path = args.file.or_else(store::last_document).ok_or(USAGE)?;
    let path = std::fs::canonicalize(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let (mut config, warning) = Config::load();
    // A move that failed would otherwise only reach stderr, which nobody sees from a launcher.
    let warning = adoption.into_iter().chain(warning).reduce(|a, b| format!("{a}; {b}"));
    if let Some(on) = args.fullscreen {
        config.view.fullscreen = on;
    }
    if let Some(on) = args.statusbar {
        config.statusbar.visible = on;
    }
    let doc = Doc::open(&path, args.password.as_deref(), config.active_theme(), config.view.page_gap)?;
    let title = format!("{} - yavcpdf", doc.file_name());
    let (width, height) = args.geometry.map_or((1280, 800), |g| g.size);
    // Sizes and offsets are capped at 16384 by parse_geometry, so they are exact in f32.
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(title)
        .with_app_id("yavcpdf")
        .with_decorations(config.view.decorations)
        .with_inner_size([width as f32, height as f32])
        .with_fullscreen(config.view.fullscreen);
    if let Some((x, y)) = args.geometry.and_then(|g| g.position) {
        viewport = viewport.with_position([x as f32, y as f32]);
    }
    let options = eframe::NativeOptions { viewport, renderer: eframe::Renderer::Glow, ..Default::default() };
    let page = args.page;
    eframe::run_native("yavcpdf", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, config, doc, page, warning))))).map_err(|e| e.to_string())
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("yavcpdf: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    if args.help {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("yavcpdf: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args, String> {
        parse_args(args.iter().map(ToString::to_string))
    }

    #[test]
    fn arguments_parse() {
        let args = parse(&["--page", "5", "doc.pdf"]).unwrap();
        assert_eq!(args.page, Some(4));
        assert_eq!(args.file, Some(PathBuf::from("doc.pdf")));
        assert!(parse(&["--help"]).unwrap().help);
        assert_eq!(parse(&[]).unwrap(), Args::default());
    }

    #[test]
    fn mistakes_are_refused() {
        assert!(parse(&["--page"]).is_err());
        assert!(parse(&["--page", "x"]).is_err());
        assert!(parse(&["--nope"]).is_err());
        assert!(parse(&["a.pdf", "b.pdf"]).is_err());
    }

    #[test]
    fn geometry_parses() {
        let size = |w, h| Geometry { size: (w, h), position: None };
        assert_eq!(parse(&["--geometry", "1000x700"]).unwrap().geometry, Some(size(1000, 700)));
        assert_eq!(parse(&["--geometry", "1000x700+40+30"]).unwrap().geometry, Some(Geometry { size: (1000, 700), position: Some((40, 30)) }));
        assert_eq!(parse_geometry("16384x1+0+0").unwrap().position, Some((0, 0)));
        assert_eq!(parse_geometry("1000x700+16384+16384").unwrap().position, Some((16384, 16384)));
        assert_eq!(parse(&[]).unwrap().geometry, None);
    }

    #[test]
    fn bad_geometry_is_refused() {
        assert!(parse(&["--geometry"]).is_err());
        for bad in [
            "0x700",
            "1000x0",
            "x700",
            "1000x",
            "abc",
            "",
            "1000",
            "1000x700+40",
            "1000x700+40+",
            "1000x700+40+30+5",
            "1000x700+40+30x",
            "1000x700+a+b",
            "1000x700-10-10",
            "1000x700+-10+5",
            "-10-10",
            "-1000x700",
            "16385x700",
            "1000x16385",
            "1000x700+99999+0",
            "1000x700+0+16385",
            "99999999999x700",
            "nanxinf",
            "1e3x700",
            "1000x700+1.5+2",
            "1000x700 ",
            "+1000x700",
            "1000X700",
        ] {
            assert!(parse_geometry(bad).is_err(), "{bad:?} should be refused");
        }
        assert!(parse_geometry("-10-10").unwrap_err().contains("not supported"));
    }

    #[test]
    fn startup_toggles_parse() {
        let args = parse(&[]).unwrap();
        assert_eq!((args.fullscreen, args.statusbar), (None, None));
        assert_eq!(parse(&["--fullscreen"]).unwrap().fullscreen, Some(true));
        assert_eq!(parse(&["--no-fullscreen"]).unwrap().fullscreen, Some(false));
        assert_eq!(parse(&["--fullscreen", "--no-fullscreen"]).unwrap().fullscreen, Some(false));
        assert_eq!(parse(&["--no-fullscreen", "--fullscreen"]).unwrap().fullscreen, Some(true));
        assert_eq!(parse(&["--statusbar"]).unwrap().statusbar, Some(true));
        assert_eq!(parse(&["--no-statusbar"]).unwrap().statusbar, Some(false));
        assert_eq!(parse(&["--statusbar", "--no-statusbar"]).unwrap().statusbar, Some(false));
        assert_eq!(parse(&["--no-statusbar", "--statusbar", "a.pdf"]).unwrap().statusbar, Some(true));
    }
}
