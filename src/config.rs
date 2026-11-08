//! The configuration: built-in defaults with the user's TOML merged over them.
use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Deserialize;

use crate::keymap::Action;

const DEFAULTS: &str = include_str!("default_config.toml");

/// A colour written `#rrggbb` or `#rrggbbaa`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct Color(pub [u8; 4]);

impl Color {
    pub fn rgb(self) -> [u8; 3] {
        [self.0[0], self.0[1], self.0[2]]
    }

    pub fn egui(self) -> egui::Color32 {
        let [r, g, b, a] = self.0;
        egui::Color32::from_rgba_unmultiplied(r, g, b, a)
    }
}

impl TryFrom<String> for Color {
    type Error = String;

    fn try_from(text: String) -> Result<Self, String> {
        let bad = || format!("\"{text}\" is not a #rrggbb or #rrggbbaa colour");
        let hex = text.strip_prefix('#').filter(|h| h.is_ascii()).ok_or_else(bad)?;
        if hex.len() != 6 && hex.len() != 8 {
            return Err(bad());
        }
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| bad());
        Ok(Self([byte(0)?, byte(2)?, byte(4)?, if hex.len() == 8 { byte(6)? } else { 255 }]))
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Theme {
    pub background: Color,
    pub page: Color,
    pub page_text: Color,
    pub recolor: bool,
    pub ui_background: Color,
    pub ui_text: Color,
    pub ui_accent: Color,
    pub hint_background: Color,
    pub hint_text: Color,
    pub search: Color,
    pub search_current: Color,
    pub selection: Color,
    /// Optional, so a theme written before links were tinted still loads.
    #[serde(default = "default_link")]
    pub link: Color,
}

fn default_link() -> Color {
    Color([0, 0, 255, 85])
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewConfig {
    pub page_gap: f32,
    pub fit_width_ratio: f32,
    pub zoom_step: f32,
    pub min_zoom: f32,
    pub max_zoom: f32,
    pub scroll_step: f32,
    pub prefetch_pages: usize,
    pub texture_budget_mb: usize,
    pub smartcase: bool,
    pub decorations: bool,
    pub restore_position: bool,
    pub highlight_links: bool,
    pub fullscreen: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusConfig {
    pub visible: bool,
    pub left: String,
    pub right: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HintConfig {
    pub alphabet: String,
    pub font_size: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Top,
    Bottom,
    Left,
    Right,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HotbarConfig {
    pub position: Side,
    pub colors: Vec<Color>,
    pub stroke_width: f32,
    pub fill_size: f32,
    pub date_format: String,
    pub border: f32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortalConfig {
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OverviewConfig {
    pub width: f32,
    pub height: f32,
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub theme: String,
    pub font: String,
    pub font_size: f32,
    pub view: ViewConfig,
    pub statusbar: StatusConfig,
    pub hints: HintConfig,
    pub hotbar: HotbarConfig,
    pub overview: OverviewConfig,
    pub portal: PortalConfig,
    pub themes: BTreeMap<String, Theme>,
    pub keys: BTreeMap<Action, Vec<String>>,
}

/// Merges `over` into `base`: tables key by key, anything else replaced whole.
fn merge(base: &mut toml::Table, over: toml::Table) {
    for (key, value) in over {
        match (base.get_mut(&key), value) {
            (Some(toml::Value::Table(b)), toml::Value::Table(o)) => merge(b, o),
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
}

impl Config {
    /// The user's config file.
    pub fn path() -> PathBuf {
        let base = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()).map_or_else(|| home().join(".config"), PathBuf::from);
        base.join("yapdf/config.toml")
    }

    /// Builds the config from the text of a user file.
    pub fn load_str(user: &str) -> Result<Self, String> {
        let mut table: toml::Table = DEFAULTS.parse().map_err(|e| format!("built-in config: {e}"))?;
        let over: toml::Table = user.parse().map_err(|e: toml::de::Error| e.to_string())?;
        merge(&mut table, over);
        let config: Self = toml::Value::Table(table).try_into().map_err(|e: toml::de::Error| e.to_string())?;
        config.check()?;
        Ok(config)
    }

    fn check(&self) -> Result<(), String> {
        if !self.themes.contains_key(&self.theme) {
            return Err(format!("theme \"{}\" is not under [themes]", self.theme));
        }
        let mut letters: Vec<char> = self.hints.alphabet.chars().collect();
        letters.sort_unstable();
        letters.dedup();
        if letters.len() < 2 {
            return Err("hints.alphabet needs at least two different characters".to_owned());
        }
        if self.hotbar.colors.is_empty() {
            return Err("hotbar.colors needs at least one colour".to_owned());
        }
        if self.view.zoom_step <= 1.0 || self.view.min_zoom <= 0.0 || self.view.max_zoom < self.view.min_zoom {
            return Err("view: zoom_step must be above 1 and min_zoom..max_zoom a positive range".to_owned());
        }
        Ok(())
    }

    /// Reads the user's file. A config that can't be used falls back to the defaults and says why.
    pub fn load() -> (Self, Option<String>) {
        let path = Self::path();
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return (Self::builtin(), Some(format!("config: {e}"))),
        };
        match Self::load_str(&text) {
            Ok(config) => (config, None),
            Err(e) => (Self::builtin(), Some(format!("config: {}", first_line(&e)))),
        }
    }

    fn builtin() -> Self {
        // `the_defaults_load` proves this: the text is compiled in.
        #[allow(clippy::expect_used)]
        Self::load_str("").expect("the built-in config is valid")
    }

    pub fn active_theme(&self) -> &Theme {
        &self.themes[&self.theme]
    }
}

fn first_line(text: &str) -> &str {
    text.lines().find(|l| !l.trim().is_empty()).unwrap_or(text)
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_load() {
        let config = Config::load_str("").unwrap();
        assert_eq!(config.theme, "dark");
        assert_eq!(config.active_theme().page.rgb(), [0x24, 0x24, 0x24]);
    }

    #[test]
    fn a_user_value_overrides_one_key_and_keeps_the_rest() {
        let config = Config::load_str("[themes.dark]\npage = \"#000000\"\n[keys]\nquit = [\"Q\"]").unwrap();
        let theme = config.active_theme();
        assert_eq!(theme.page.rgb(), [0, 0, 0]);
        assert_eq!(theme.page_text.rgb(), [0xeb, 0xdb, 0xb2]);
        assert_eq!(config.keys[&Action::Quit], ["Q"]);
        assert_eq!(config.keys[&Action::ScrollDown], ["j", "<down>"]);
    }

    #[test]
    fn a_new_theme_must_be_complete_and_mistakes_are_reported() {
        assert!(Config::load_str("[themes.mine]\npage = \"#000000\"").is_err());
        assert!(Config::load_str("theme = \"none\"").is_err());
        assert!(Config::load_str("[keys]\nno_such_action = [\"x\"]").is_err());
        assert!(Config::load_str("[view]\nno_such_setting = 1").is_err());
        assert!(Config::load_str("[themes.dark]\npage = \"red\"").is_err());
    }

    #[test]
    fn colours_parse_with_and_without_alpha() {
        assert_eq!(Color::try_from("#010203".to_owned()).unwrap().0, [1, 2, 3, 255]);
        assert_eq!(Color::try_from("#01020304".to_owned()).unwrap().0, [1, 2, 3, 4]);
        assert!(Color::try_from("#12345".to_owned()).is_err());
        assert!(Color::try_from("#éééé".to_owned()).is_err());
    }
}
