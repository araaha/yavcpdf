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
    /// Optional, so a theme written before fields were tinted still loads.
    #[serde(default = "default_field")]
    pub field: Color,
}

fn default_link() -> Color {
    Color([0, 0, 255, 85])
}

fn default_field() -> Color {
    Color([250, 189, 47, 51])
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
    pub prefetch_text: bool,
    pub texture_budget_mb: usize,
    pub smartcase: bool,
    pub decorations: bool,
    pub restore_position: bool,
    pub highlight_links: bool,
    pub highlight_fields: bool,
    pub fullscreen: bool,
    pub border: f32,
    /// Where a jump puts its target: percent of the window height, 0 at the bottom to 100 at the top.
    pub jump_position: f32,
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
    /// Overrides `view.border` for the hotbar alone.
    pub border: Option<f32>,
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
        user_dir("XDG_CONFIG_HOME", ".config").join("config.toml")
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
        for (key, width) in [("view.border", Some(self.view.border)), ("hotbar.border", self.hotbar.border)] {
            if let Some(width) = width.filter(|w| !w.is_finite() || !(0.0..=8.0).contains(w)) {
                return Err(format!("{key} = {width} is out of range: use 0 to 8"));
            }
        }
        if !self.view.jump_position.is_finite() || !(0.0..=100.0).contains(&self.view.jump_position) {
            return Err(format!("view.jump_position = {} is out of range: use 0 to 100", self.view.jump_position));
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

#[cfg(not(windows))]
pub fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from)
}

#[cfg(windows)]
pub fn home() -> PathBuf {
    let set = |name| std::env::var_os(name).filter(|v| !v.is_empty());
    set("USERPROFILE").or_else(|| set("HOME")).map_or_else(|| PathBuf::from("."), PathBuf::from)
}

/// The yavcpdf folder for settings or data: under `$xdg` when that is set and not empty, else the
/// platform's usual place (`unix_base` under the home folder on Linux, `%APPDATA%` on Windows,
/// `~/Library/Application Support` on macOS).
pub fn user_dir(xdg: &str, unix_base: &str) -> PathBuf {
    let base = std::env::var_os(xdg).filter(|v| !v.is_empty()).map_or_else(|| default_base(unix_base), PathBuf::from);
    base.join("yavcpdf")
}

#[cfg(windows)]
fn default_base(_unix_base: &str) -> PathBuf {
    std::env::var_os("APPDATA").filter(|v| !v.is_empty()).map_or_else(|| home().join("AppData").join("Roaming"), PathBuf::from)
}

#[cfg(target_os = "macos")]
fn default_base(_unix_base: &str) -> PathBuf {
    home().join("Library/Application Support")
}

#[cfg(not(any(windows, target_os = "macos")))]
fn default_base(unix_base: &str) -> PathBuf {
    home().join(unix_base)
}

/// A typed path, with a leading `~/` (and `~\` on Windows) meaning the home folder.
pub fn expand_home(given: &str) -> PathBuf {
    let rest = given.strip_prefix("~/");
    #[cfg(windows)]
    let rest = rest.or_else(|| given.strip_prefix("~\\"));
    rest.map_or_else(|| PathBuf::from(given), |rest| home().join(rest))
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
    fn text_prefetching_is_on_by_default_and_can_be_turned_off() {
        assert!(Config::load_str("").unwrap().view.prefetch_text);
        assert!(!Config::load_str("[view]\nprefetch_text = false").unwrap().view.prefetch_text);
        assert!(Config::load_str("[view]\nprefetch_text = false").unwrap().view.highlight_links);
    }

    #[test]
    fn field_highlighting_is_on_by_default_and_can_be_turned_off() {
        assert!(Config::load_str("").unwrap().view.highlight_fields);
        assert!(!Config::load_str("[view]\nhighlight_fields = false").unwrap().view.highlight_fields);
        assert!(Config::load_str("[view]\nhighlight_fields = false").unwrap().view.highlight_links);
    }

    #[test]
    fn a_theme_without_a_field_colour_gets_the_default() {
        let theme = "background = \"#000000\"\npage = \"#000000\"\npage_text = \"#ffffff\"\nrecolor = false\n\
            ui_background = \"#000000\"\nui_text = \"#ffffff\"\nui_accent = \"#ffffff\"\n\
            hint_background = \"#ffffff\"\nhint_text = \"#000000\"\nsearch = \"#ffffff\"\n\
            search_current = \"#ffffff\"\nselection = \"#ffffff\"\n";
        let config = Config::load_str(&format!("theme = \"mine\"\n[themes.mine]\n{theme}")).unwrap();
        let mine = config.active_theme();
        assert_eq!(mine.field.0, [250, 189, 47, 51]);
        assert_eq!(mine.link.0, [0, 0, 255, 85]);
        let config = Config::load_str(&format!("theme = \"mine\"\n[themes.mine]\n{theme}field = \"#11223344\"")).unwrap();
        assert_eq!(config.active_theme().field.0, [0x11, 0x22, 0x33, 0x44]);
        assert_eq!(Config::load_str("").unwrap().active_theme().field.0, [0xfa, 0xbd, 0x2f, 0x33]);
    }

    #[test]
    fn the_border_is_shared_and_the_hotbar_can_override_it() {
        let config = Config::load_str("").unwrap();
        assert_eq!((config.view.border, config.hotbar.border), (1.0, None));
        let config = Config::load_str("[view]\nborder = 3.0").unwrap();
        assert_eq!((config.view.border, config.hotbar.border), (3.0, None));
        let config = Config::load_str("[hotbar]\nborder = 2.0").unwrap();
        assert_eq!((config.view.border, config.hotbar.border), (1.0, Some(2.0)));
        assert!(Config::load_str("[view]\nborder = 0").is_ok());
        assert!(Config::load_str("[view]\nborder = 8").is_ok());
        let error = Config::load_str("[view]\nborder = 25").unwrap_err().to_string();
        assert!(error.contains("view.border = 25 is out of range: use 0 to 8"), "{error}");
    }

    #[test]
    fn the_jump_position_defaults_to_the_middle_and_must_be_a_percentage() {
        assert_eq!(Config::load_str("").unwrap().view.jump_position, 50.0);
        assert_eq!(Config::load_str("[view]\njump_position = 0").unwrap().view.jump_position, 0.0);
        assert!(Config::load_str("[view]\njump_position = 100").is_ok());
        let error = Config::load_str("[view]\njump_position = 101").unwrap_err().to_string();
        assert!(error.contains("view.jump_position = 101 is out of range: use 0 to 100"), "{error}");
        assert!(Config::load_str("[view]\njump_position = -1").is_err());
    }

    #[test]
    fn jump_position_parsing_and_rejection() {
        let pos = |v: &str| Config::load_str(&format!("[view]\njump_position = {v}"));
        assert_eq!(pos("0").unwrap().view.jump_position, 0.0);
        assert_eq!(pos("0.0").unwrap().view.jump_position, 0.0);
        assert_eq!(pos("100").unwrap().view.jump_position, 100.0);
        assert_eq!(pos("100.0").unwrap().view.jump_position, 100.0);
        assert_eq!(pos("30").unwrap().view.jump_position, 30.0);
        assert_eq!(pos("30.0").unwrap().view.jump_position, 30.0);
        assert_eq!(pos("33.5").unwrap().view.jump_position, 33.5);
        for (bad, shown) in [
            ("-0.1", "-0.1"),
            ("100.1", "100.1"),
            ("nan", "NaN"),
            ("-nan", "NaN"),
            ("inf", "inf"),
            ("-inf", "-inf"),
            ("1e30", "1000000000000000000000000000000"),
        ] {
            let error = pos(bad).unwrap_err();
            assert_eq!(error, format!("view.jump_position = {shown} is out of range: use 0 to 100"), "{bad}");
        }
        assert!(pos("\"50\"").is_err());
    }

    #[test]
    fn a_bad_border_is_reported() {
        for bad in ["-1.0", "9.0", "nan", "inf"] {
            assert!(Config::load_str(&format!("[view]\nborder = {bad}")).is_err(), "view {bad}");
            assert!(Config::load_str(&format!("[hotbar]\nborder = {bad}")).is_err(), "hotbar {bad}");
        }
    }

    #[test]
    fn colours_parse_with_and_without_alpha() {
        assert_eq!(Color::try_from("#010203".to_owned()).unwrap().0, [1, 2, 3, 255]);
        assert_eq!(Color::try_from("#01020304".to_owned()).unwrap().0, [1, 2, 3, 4]);
        assert!(Color::try_from("#12345".to_owned()).is_err());
        assert!(Color::try_from("#éééé".to_owned()).is_err());
    }
}

#[cfg(all(test, not(any(windows, target_os = "macos"))))]
mod port_tests {
    use super::*;

    #[test]
    fn expand_home_cases() {
        let h = home();
        assert_eq!(expand_home("~"), PathBuf::from("~"));
        assert_eq!(expand_home("~/x"), h.join("x"));
        assert_eq!(expand_home("~/"), h.join(""));
        assert_eq!(expand_home("~/a/b"), h.join("a/b"));
        assert_eq!(expand_home("~user"), PathBuf::from("~user"));
        assert_eq!(expand_home("~user/x"), PathBuf::from("~user/x"));
        assert_eq!(expand_home("/abs"), PathBuf::from("/abs"));
        assert_eq!(expand_home("rel"), PathBuf::from("rel"));
        assert_eq!(expand_home(""), PathBuf::from(""));
        // Backslash stays literal on Linux.
        assert_eq!(expand_home("~\\x"), PathBuf::from("~\\x"));
    }

    #[test]
    fn home_is_the_home_variable_or_dot() {
        let expected = std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from);
        assert_eq!(home(), expected);
    }

    #[test]
    fn the_linux_default_base_is_under_home() {
        assert_eq!(default_base(".config"), home().join(".config"));
        assert_eq!(default_base(".local/share"), home().join(".local/share"));
    }

    #[test]
    fn user_dir_ends_in_yavcpdf_and_follows_an_unset_variable() {
        let unset = "YAVCPDF_TEST_SURELY_UNSET_VARIABLE";
        assert_eq!(user_dir(unset, ".config"), home().join(".config/yavcpdf"));
        assert_eq!(user_dir(unset, ".local/share"), home().join(".local/share/yavcpdf"));
        // user_dir reads the environment itself and takes no values, so the set and empty
        // cases are covered by the binary runs, not here.
    }
}
