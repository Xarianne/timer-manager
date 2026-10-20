//! Look and feel: DMS colors, fonts, Material icons and the custom widgets.

use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::SystemTime,
};

use eframe::egui::{
    self, text::LayoutJob, Align, Align2, Button, Color32, CornerRadius, FontFamily, FontId, Layout,
    Response, RichText, Sense, Stroke, StrokeKind, TextFormat, Ui,
};
use serde_json::Value;

const DMS_FONTS: &str = "/usr/share/quickshell/dms/DankCommon/assets/fonts";
static ICONS_OK: AtomicBool = AtomicBool::new(false);

/// Material Symbols codepoints.
pub mod icon {
    pub const SCHEDULE: char = '\u{e8b5}';
    pub const ADD: char = '\u{e145}';
    pub const REFRESH: char = '\u{e5d5}';
    pub const PLAY: char = '\u{e037}';
    pub const EDIT: char = '\u{e3c9}';
    pub const DELETE: char = '\u{e872}';
    pub const CLOSE: char = '\u{e5cd}';
    pub const FOLDER: char = '\u{e2c8}';
    pub const ARTICLE: char = '\u{ef42}';
    pub const TUNE: char = '\u{e429}';
    pub const TERMINAL: char = '\u{eb8e}';
    pub const BACK: char = '\u{e5c4}';
    pub const LIST: char = '\u{e896}';
    pub const CHECK: char = '\u{e5ca}';
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    pub background: Color32,
    /// How opaque the window background is; cards stay solid.
    pub window_alpha: f32,
    /// Card fill.
    pub surface: Color32,
    /// Inputs and buttons.
    pub surface_high: Color32,
    pub primary: Color32,
    pub on_primary: Color32,
    pub primary_container: Color32,
    pub on_primary_container: Color32,
    pub on_surface: Color32,
    pub on_surface_variant: Color32,
    pub outline: Color32,
    pub outline_variant: Color32,
    pub error: Color32,
    pub success: Color32,
}

impl Palette {
    /// The see-through window background.
    pub fn backdrop(&self) -> Color32 {
        self.background.gamma_multiply(self.window_alpha)
    }

    fn fallback() -> Palette {
        let c = |hex: u32| Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
        Palette {
            dark: true,
            background: c(0x1e1e2e),
            window_alpha: 0.92,
            surface: c(0x28283a),
            surface_high: c(0x33334a),
            primary: c(0x89b4fa),
            on_primary: c(0x1e1e2e),
            primary_container: c(0x2f4f78),
            on_primary_container: c(0xd6e6ff),
            on_surface: c(0xcdd6f4),
            on_surface_variant: c(0xa6adc8),
            outline: c(0x7f849c),
            outline_variant: c(0x45475a),
            error: c(0xf38ba8),
            success: c(0xa6e3a1),
        }
    }
}

fn colors_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(".cache/DankMaterialShell/dms-colors.json")
}

/// When DMS last rewrote its theme file.
pub fn colors_modified() -> Option<SystemTime> {
    fs::metadata(colors_path()).ok()?.modified().ok()
}

fn hex(v: &Value) -> Option<Color32> {
    let s = v.as_str()?.trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(s, 16).ok()?;
    Some(Color32::from_rgb((n >> 16) as u8, (n >> 8) as u8, n as u8))
}

/// Whether the window should get the compositor's title bar. Tiling compositors draw their own
/// borders, but floating desktops like Plasma need it to move and close the window.
pub fn use_window_decorations() -> bool {
    const TILING: [&str; 6] = ["hyprland", "niri", "sway", "river", "scroll", "mango"];
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_lowercase();
    !desktop.split(':').any(|d| TILING.contains(&d))
}

/// Reads DMS's live theme; falls back to a built-in dark palette.
pub fn load_palette() -> Palette {
    let mut p = Palette::fallback();
    let Ok(text) = fs::read_to_string(colors_path()) else { return p };
    let Ok(json) = serde_json::from_str::<Value>(&text) else { return p };
    let mode = json["mode"].as_str().unwrap_or("dark").to_string();
    p.dark = mode != "light";
    let colors = &json["colors"][mode.as_str()];
    let set = |key: &str, slot: &mut Color32| {
        if let Some(c) = hex(&colors[key]) {
            *slot = c;
        }
    };
    set("background", &mut p.background);
    set("surface_container_high", &mut p.surface);
    set("surface_container_highest", &mut p.surface_high);
    set("primary", &mut p.primary);
    set("on_primary", &mut p.on_primary);
    set("primary_container", &mut p.primary_container);
    set("on_primary_container", &mut p.on_primary_container);
    set("on_surface", &mut p.on_surface);
    set("on_surface_variant", &mut p.on_surface_variant);
    set("outline", &mut p.outline);
    set("outline_variant", &mut p.outline_variant);
    set("error", &mut p.error);
    if let Some(c) = hex(&json["dank16"]["color2"][mode.as_str()]) {
        p.success = c;
    }
    p
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

pub fn apply_palette(ctx: &egui::Context, p: &Palette) {
    let mut v = if p.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = p.backdrop();
    v.window_fill = p.surface;
    v.extreme_bg_color = p.surface_high;
    v.faint_bg_color = p.surface;
    v.code_bg_color = p.surface_high;
    v.window_stroke = Stroke::new(1.0, p.outline_variant);
    v.window_corner_radius = CornerRadius::same(14);
    v.menu_corner_radius = CornerRadius::same(10);
    v.selection.bg_fill = p.primary_container;
    v.selection.stroke = Stroke::new(1.0, p.primary);
    v.hyperlink_color = p.primary;
    v.error_fg_color = p.error;

    let r = CornerRadius::same(8);
    let edge = Stroke::new(1.0, p.outline_variant.gamma_multiply(0.7));
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.surface;
    w.noninteractive.weak_bg_fill = p.surface;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.outline_variant.gamma_multiply(0.6));
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.on_surface);
    w.inactive.bg_fill = p.surface_high;
    w.inactive.weak_bg_fill = p.surface_high;
    w.inactive.bg_stroke = edge;
    w.inactive.fg_stroke = Stroke::new(1.0, p.on_surface);
    let hover = mix(p.surface_high, p.on_surface, 0.09);
    w.hovered.bg_fill = hover;
    w.hovered.weak_bg_fill = hover;
    w.hovered.bg_stroke = Stroke::new(1.0, p.primary.gamma_multiply(0.6));
    w.hovered.fg_stroke = Stroke::new(1.0, p.on_surface);
    let active = mix(p.surface_high, p.primary, 0.28);
    w.active.bg_fill = active;
    w.active.weak_bg_fill = active;
    w.active.bg_stroke = Stroke::new(1.0, p.primary);
    w.active.fg_stroke = Stroke::new(1.0, p.on_surface);
    w.open.bg_fill = p.surface_high;
    w.open.weak_bg_fill = p.surface_high;
    w.open.bg_stroke = edge;
    for state in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        state.corner_radius = r;
    }
    ctx.all_styles_mut(|style| style.visuals = v.clone());
}

pub fn apply_text_styles(ctx: &egui::Context) {
    use egui::TextStyle::*;
    ctx.all_styles_mut(|style| {
        style.text_styles.insert(Heading, FontId::new(17.0, FontFamily::Name("bold".into())));
        style.text_styles.insert(Body, FontId::proportional(14.0));
        style.text_styles.insert(Button, FontId::proportional(14.0));
        style.text_styles.insert(Small, FontId::proportional(12.0));
        style.text_styles.insert(Monospace, FontId::monospace(13.0));
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(12.0, 5.0);
        style.spacing.interact_size = egui::vec2(36.0, 28.0);
        style.spacing.scroll = egui::style::ScrollStyle::floating();
    });
}

/// Loads an installed font through fontconfig; None if it isn't installed.
fn system_font(family: &str, bold: bool) -> Option<Vec<u8>> {
    let pattern = if bold { format!("{family}:bold") } else { family.to_string() };
    let out = std::process::Command::new("fc-match")
        .args(["-f", "%{family}|%{file}", &pattern])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let (found, path) = text.split_once('|')?;
    // fc-match substitutes something else when the font is missing.
    if !found.split(',').any(|f| f.trim() == family) {
        return None;
    }
    if bold && !path.to_lowercase().contains("bold") {
        return None;
    }
    fs::read(path.trim()).ok()
}

/// Inter and Material Symbols come from the DMS install, Noto Sans from fontconfig.
/// Anything missing falls back to egui's built-in fonts.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let add = |fonts: &mut egui::FontDefinitions, name: &str, bytes: Vec<u8>| {
        fonts
            .font_data
            .insert(name.to_owned(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
    };

    let mut body = Vec::new();
    if let Ok(bytes) = fs::read(format!("{DMS_FONTS}/inter/InterVariable.ttf")) {
        add(&mut fonts, "inter", bytes);
        body.push("inter".to_owned());
    }
    if let Some(bytes) = system_font("Noto Sans", false) {
        add(&mut fonts, "noto-sans", bytes);
        body.push("noto-sans".to_owned());
    }
    let mut bold = Vec::new();
    if let Some(bytes) = system_font("Noto Sans", true) {
        add(&mut fonts, "noto-sans-bold", bytes);
        bold.push("noto-sans-bold".to_owned());
    }
    let icons_path =
        format!("{DMS_FONTS}/material-design-icons/variablefont/MaterialSymbolsRounded[FILL,GRAD,opsz,wght].ttf");
    let icons = match fs::read(&icons_path) {
        Ok(bytes) => {
            add(&mut fonts, "material-symbols", bytes);
            ICONS_OK.store(true, Ordering::Relaxed);
            Some(vec!["material-symbols".to_owned()])
        }
        Err(_) => None,
    };

    let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
    for (i, name) in body.into_iter().enumerate() {
        proportional.insert(i, name);
    }
    bold.extend(proportional.clone());
    let fallback = proportional.clone();
    fonts.families.insert(FontFamily::Name("bold".into()), bold);
    fonts
        .families
        .insert(FontFamily::Name("icons".into()), icons.unwrap_or(fallback));
    ctx.set_fonts(fonts);
}

fn icons_family() -> FontFamily {
    FontFamily::Name("icons".into())
}

fn icons_ok() -> bool {
    ICONS_OK.load(Ordering::Relaxed)
}

/// An icon as text; empty if the icon font isn't installed, so the UI still works.
pub fn icon(glyph: char, size: f32) -> RichText {
    if icons_ok() {
        RichText::new(glyph.to_string()).family(icons_family()).size(size)
    } else {
        RichText::new("")
    }
}

fn labeled(glyph: Option<char>, label: &str, color: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    let mut gap = 0.0;
    if let (Some(g), true) = (glyph, icons_ok()) {
        job.append(
            &g.to_string(),
            0.0,
            TextFormat { font_id: FontId::new(18.0, icons_family()), color, valign: Align::Center, ..Default::default() },
        );
        gap = 8.0;
    }
    if !label.is_empty() {
        job.append(
            label,
            gap,
            TextFormat { font_id: FontId::proportional(14.0), color, valign: Align::Center, ..Default::default() },
        );
    }
    job
}

/// A rounded section card.
pub fn card<R>(ui: &mut Ui, p: &Palette, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(p.surface)
        .corner_radius(CornerRadius::same(14))
        .inner_margin(egui::Margin::symmetric(16, 11))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// A small rounded badge.
pub fn pill(ui: &mut Ui, p: &Palette, text: &str) {
    egui::Frame::new()
        .fill(p.primary_container)
        .corner_radius(CornerRadius::same(255))
        .inner_margin(egui::Margin::symmetric(9, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).small().color(p.on_primary_container));
        });
}

pub fn section_header(ui: &mut Ui, p: &Palette, glyph: char, title: &str) {
    ui.horizontal(|ui| {
        ui.label(icon(glyph, 20.0).color(p.primary));
        ui.heading(title);
    });
    ui.add_space(2.0);
}

pub fn divider(ui: &mut Ui) {
    ui.add_space(1.0);
    ui.separator();
    ui.add_space(1.0);
}

/// A title and grey description on the left, the control on the right.
pub fn setting_row(ui: &mut Ui, p: &Palette, title: &str, desc: &str, control: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        let left = (ui.available_width() * 0.5).max(180.0);
        ui.vertical(|ui| {
            ui.set_width(left);
            ui.label(RichText::new(title).size(14.0).color(p.on_surface));
            if !desc.is_empty() {
                ui.label(RichText::new(desc).small().color(p.on_surface_variant));
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), control);
    });
}

pub fn toggle(ui: &mut Ui, p: &Palette, on: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(46.0, 26.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_responsive(response.id, on);
        let radius = rect.height() / 2.0;
        let track = mix(p.surface_high, p.primary, t);
        ui.painter().rect_filled(rect, radius, track);
        ui.painter().rect_stroke(
            rect,
            radius,
            Stroke::new(1.0, mix(p.outline_variant, p.primary, t)),
            StrokeKind::Inside,
        );
        let x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
        let knob = mix(p.outline, p.on_primary, t);
        ui.painter()
            .circle_filled(egui::pos2(x, rect.center().y), egui::lerp(8.0..=10.0, t), knob);
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A sidebar entry with an icon; the selected one gets the accent pill.
pub fn nav_item(ui: &mut Ui, p: &Palette, glyph: char, label: &str, selected: bool) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 36.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let (bg, fg) = if selected {
            (p.primary_container, p.on_primary_container)
        } else if response.hovered() {
            (mix(p.background, p.on_surface, 0.07), p.on_surface)
        } else {
            (Color32::TRANSPARENT, p.on_surface_variant)
        };
        ui.painter().rect_filled(rect, CornerRadius::same(10), bg);
        if icons_ok() {
            ui.painter().text(
                rect.left_center() + egui::vec2(14.0, 0.0),
                Align2::LEFT_CENTER,
                glyph.to_string(),
                FontId::new(21.0, icons_family()),
                fg,
            );
        }
        let x = if icons_ok() { 46.0 } else { 14.0 };
        ui.painter().text(
            rect.left_center() + egui::vec2(x, 0.0),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(14.5),
            fg,
        );
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn action_button(ui: &mut Ui, p: &Palette, glyph: char, label: &str, danger: bool) -> Response {
    let color = if danger { p.error } else { p.on_surface };
    ui.add(
        Button::new(labeled(Some(glyph), label, color))
            .fill(p.surface_high)
            .corner_radius(CornerRadius::same(8))
            .min_size(egui::vec2(0.0, 30.0)),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn primary_button(ui: &mut Ui, p: &Palette, glyph: char, label: &str) -> Response {
    ui.add(
        Button::new(labeled(Some(glyph), label, p.on_primary))
            .fill(p.primary)
            .corner_radius(CornerRadius::same(8))
            .min_size(egui::vec2(0.0, 32.0)),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn icon_button(ui: &mut Ui, p: &Palette, glyph: char, tip: &str) -> Response {
    ui.add(
        Button::new(labeled(Some(glyph), "", p.on_surface_variant))
            .fill(Color32::TRANSPARENT)
            .corner_radius(CornerRadius::same(10))
            .min_size(egui::vec2(32.0, 32.0)),
    )
    .on_hover_text(tip)
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}
