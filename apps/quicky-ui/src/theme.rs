//! The active Omarchy theme, as GTK CSS.
//!
//! Colors come from the theme's `colors.toml` and corner rounding from
//! Hyprland, the same sources the Omarchy shell uses, so the editors match
//! Quicky and the rest of the desktop.

use gtk::gdk;
use std::{fs, process::Command};

#[derive(Clone, Debug)]
pub struct Palette {
    pub background: String,
    pub foreground: String,
    pub accent: String,
    pub urgent: String,
    pub rounding: i32,
}

impl Palette {
    pub fn load() -> Self {
        let path = format!(
            "{}/.local/state/omarchy/current/theme/colors.toml",
            std::env::var("HOME").unwrap_or_default()
        );
        let text = fs::read_to_string(path).unwrap_or_default();
        let get = |key: &str| {
            text.lines().find_map(|line| {
                let (k, v) = line.split_once('=')?;
                if k.trim() != key {
                    return None;
                }
                let v = v.trim().trim_matches(|c| c == '"' || c == '\'');
                (v.starts_with('#') && v.len() >= 7).then(|| v[..7].to_string())
            })
        };
        Palette {
            background: get("background").or_else(|| get("color0")).unwrap_or("#101315".into()),
            foreground: get("foreground").or_else(|| get("color7")).unwrap_or("#cacccc".into()),
            accent: get("accent").or_else(|| get("color4")).unwrap_or("#cacccc".into()),
            urgent: get("red").or_else(|| get("color1")).unwrap_or("#a55555".into()),
            rounding: hypr_rounding(),
        }
    }

    pub fn rgba(hex: &str, alpha: f32) -> gdk::RGBA {
        let mut c = gdk::RGBA::parse(hex).unwrap_or(gdk::RGBA::WHITE);
        c.set_alpha(alpha);
        c
    }

    fn css(&self) -> String {
        let (bg, fg, acc, red, r) = (
            &self.background,
            &self.foreground,
            &self.accent,
            &self.urgent,
            self.rounding,
        );
        format!(
            r#"
window, .quicky {{ background-color: {bg}; color: {fg}; font-family: monospace; font-size: 12px; }}
* {{ border-radius: {r}px; outline: none; }}
.title {{ font-weight: bold; font-size: 14px; }}
.dim {{ opacity: 0.6; }}
.accent {{ color: {acc}; }}
.video {{ background-color: #000; border: 1px solid alpha({fg}, 0.15); }}
separator {{ background-color: alpha({fg}, 0.15); min-height: 1px; }}
button {{
  background: alpha({fg}, 0.04); background-image: none; box-shadow: none; text-shadow: none;
  border: 1px solid alpha({fg}, 0.25); color: {fg}; padding: 4px 12px; min-height: 22px;
}}
button:hover {{ background: alpha({fg}, 0.10); }}
button:active, button:checked {{ background: alpha({acc}, 0.22); border-color: {acc}; }}
button:disabled {{ opacity: 0.4; }}
button.suggested-action {{ border-color: {acc}; color: {acc}; background: alpha({acc}, 0.12); }}
button.suggested-action:hover {{ background: alpha({acc}, 0.24); }}
button.destructive-action {{ border-color: {red}; color: {red}; }}
button.flat {{ border-color: transparent; background: transparent; }}
button.flat:hover {{ background: alpha({fg}, 0.10); }}
entry {{
  background: alpha({fg}, 0.04); box-shadow: none; color: {fg};
  border: 1px solid alpha({fg}, 0.25); min-height: 22px; caret-color: {acc};
}}
entry:focus-within {{ border-color: {acc}; }}
entry selection {{ background: alpha({acc}, 0.35); }}
checkbutton check {{ border: 1px solid alpha({fg}, 0.45); background: transparent; box-shadow: none; }}
checkbutton check:checked {{ background: {acc}; color: {bg}; border-color: {acc}; }}
.section {{ font-weight: bold; opacity: 0.8; }}
scale trough {{ background: alpha({fg}, 0.15); border: none; min-height: 4px; }}
scale highlight {{ background: {acc}; border: none; margin: 0; }}
scale slider {{ background: {acc}; border: none; box-shadow: none; min-width: 12px; min-height: 12px; margin: -5px; }}
scale marks, scale mark indicator {{ color: alpha({fg}, 0.5); }}
spinbutton {{ background: alpha({fg}, 0.04); border: 1px solid alpha({fg}, 0.25); box-shadow: none; color: {fg}; }}
spinbutton button {{ border: none; background: transparent; min-height: 18px; padding: 0 6px; }}
popover contents, popover arrow {{ background: {bg}; border: 1px solid alpha({fg}, 0.25); }}
button.compact {{ padding: 2px 4px; min-width: 0; }}
.tile {{ border: 1px solid alpha({fg}, 0.25); padding: 2px; }}
.tile.chosen {{ border-color: {acc}; }}
.badge {{ background: alpha({bg}, 0.8); color: {fg}; padding: 0 4px; font-size: 10px; }}
.swatch {{ min-width: 26px; min-height: 26px; padding: 0; border: 1px solid alpha({fg}, 0.25); }}
.swatch:checked {{ border: 2px solid {acc}; }}
"#
        )
    }

    /// Applies the theme to every window of this process.
    pub fn install(&self) {
        let provider = gtk::CssProvider::new();
        provider.load_from_string(&self.css());
        if let Some(display) = gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_USER,
            );
        }
    }
}

// Hyprland's `decoration:rounding`; 0 (sharp) when Hyprland isn't reachable.
fn hypr_rounding() -> i32 {
    let out = Command::new("hyprctl")
        .args(["-j", "getoption", "decoration:rounding"])
        .output();
    let Ok(out) = out else { return 0 };
    let text = String::from_utf8_lossy(&out.stdout);
    text.split("\"int\":")
        .nth(1)
        .and_then(|rest| {
            rest.trim()
                .split(|c: char| !c.is_ascii_digit())
                .next()?
                .parse()
                .ok()
        })
        .unwrap_or(0)
}
