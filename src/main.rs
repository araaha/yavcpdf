//! yapdf: a minimal, keyboard-driven PDF viewer on the PdfCraft render engine.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod actions;
mod app;
mod cache;
mod chrome;
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

const USAGE: &str = "usage: yapdf [--page N] [--password PW] [file.pdf]

With no file, the document opened last is opened again.
The configuration is ~/.config/yapdf/config.toml.";

#[derive(Debug, Default, PartialEq)]
struct Args {
    file: Option<PathBuf>,
    page: Option<usize>,
    password: Option<String>,
    help: bool,
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
    store::adopt_old_folders();
    let path = args.file.or_else(store::last_document).ok_or(USAGE)?;
    let path = std::fs::canonicalize(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let (config, warning) = Config::load();
    let doc = Doc::open(&path, args.password.as_deref(), config.active_theme(), config.view.page_gap)?;
    let title = format!("{} - yapdf", doc.file_name());
    let viewport = egui::ViewportBuilder::default()
        .with_title(title)
        .with_app_id("yapdf")
        .with_decorations(config.view.decorations)
        .with_inner_size([1280.0, 800.0]);
    let options = eframe::NativeOptions { viewport, renderer: eframe::Renderer::Glow, ..Default::default() };
    let page = args.page;
    eframe::run_native("yapdf", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, config, doc, page, warning))))).map_err(|e| e.to_string())
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("yapdf: {e}\n{USAGE}");
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
            eprintln!("yapdf: {e}");
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
}
