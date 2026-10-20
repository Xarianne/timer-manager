mod systemd;
mod theme;

use std::{
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant, SystemTime},
};

use chrono::{Local, TimeZone};
use eframe::egui::{self, Align, Layout, RichText};
use systemd::{Kind, Restart, ServiceType, Spec, TimerInfo, WEEKDAYS};
use theme::{icon, Palette};

enum View {
    List,
    Edit { spec: Spec, existing: bool },
    Logs { name: String, text: String },
}

#[derive(PartialEq, Clone, Copy)]
enum Page {
    Mine,
    All,
    New,
}

struct App {
    timers: Vec<TimerInfo>,
    show_all: bool,
    view: View,
    status: String,
    search: String,
    confirm_delete: Option<String>,
    last_refresh: Instant,
    preview: (String, Result<String, String>),
    running: Option<(String, Receiver<String>)>,
    picker: Option<Receiver<Result<Option<String>, String>>>,
    palette: Palette,
    palette_mtime: Option<SystemTime>,
    last_theme_check: Instant,
}

impl App {
    fn new(palette: Palette) -> Self {
        let mut app = App {
            timers: Vec::new(),
            show_all: false,
            view: View::List,
            status: String::new(),
            search: String::new(),
            confirm_delete: None,
            last_refresh: Instant::now(),
            preview: (String::new(), Ok(String::new())),
            running: None,
            picker: None,
            palette,
            palette_mtime: theme::colors_modified(),
            last_theme_check: Instant::now(),
        };
        app.refresh();
        let args: Vec<String> = std::env::args().collect();
        if args.iter().any(|a| a == "--new") {
            app.go(Page::New);
        } else if args.iter().any(|a| a == "--all") {
            app.go(Page::All);
        }
        app
    }

    fn refresh(&mut self) {
        self.timers = systemd::list_timers(self.show_all);
        self.last_refresh = Instant::now();
    }

    fn report(&mut self, result: Result<String, String>) {
        self.status = match result {
            Ok(msg) => msg,
            Err(e) => format!("Error: {e}"),
        };
        self.refresh();
    }

    fn page(&self) -> Page {
        match &self.view {
            View::Edit { existing: false, .. } => Page::New,
            _ if self.show_all => Page::All,
            _ => Page::Mine,
        }
    }

    fn go(&mut self, page: Page) {
        match page {
            Page::Mine | Page::All => {
                self.show_all = page == Page::All;
                self.view = View::List;
                self.refresh();
            }
            Page::New => self.view = View::Edit { spec: Spec::default(), existing: false },
        }
    }

    /// DMS rewrites its colors when the theme changes; follow along.
    fn poll_theme(&mut self, ctx: &egui::Context) {
        if self.last_theme_check.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.last_theme_check = Instant::now();
        let modified = theme::colors_modified();
        if modified != self.palette_mtime {
            self.palette_mtime = modified;
            self.palette = theme::load_palette();
            theme::apply_palette(ctx, &self.palette);
        }
    }
}

/// Shows the desktop's file chooser (KDE's, else GTK's) and returns the chosen path.
fn pick_file() -> Result<Option<String>, String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let dialogs: [(&str, Vec<&str>); 2] = [
        ("kdialog", vec!["--getopenfilename", &home]),
        ("zenity", vec!["--file-selection"]),
    ];
    for (program, args) in dialogs {
        match std::process::Command::new(program).args(&args).output() {
            Ok(out) => {
                let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
                return Ok(if path.is_empty() { None } else { Some(path) });
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("{program}: {e}")),
        }
    }
    Err("No file chooser found; install kdialog or zenity".into())
}

fn format_time(us: Option<u64>) -> String {
    let Some(us) = us else { return "-".into() };
    let Some(t) = Local.timestamp_micros(us as i64).single() else {
        return "-".into();
    };
    let secs = (t - Local::now()).num_seconds();
    let (n, future) = (secs.abs(), secs >= 0);
    let rel = match n {
        0..=59 => format!("{n}s"),
        60..=3599 => format!("{}m", n / 60),
        3600..=86399 => format!("{}h {}m", n / 3600, n % 3600 / 60),
        _ => format!("{}d {}h", n / 86400, n % 86400 / 3600),
    };
    let rel = if future { format!("in {rel}") } else { format!("{rel} ago") };
    format!("{} ({rel})", t.format("%a %d %b %H:%M"))
}

impl eframe::App for App {
    /// Fully transparent, so only the tinted panels (and the solid cards) are drawn.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        ctx.request_repaint_after(Duration::from_secs(2));
        self.poll_theme(&ctx);
        if matches!(self.view, View::List) && self.last_refresh.elapsed() > Duration::from_secs(5) {
            self.refresh();
        }

        if let Some(rx) = &self.picker {
            match rx.try_recv() {
                Ok(result) => {
                    self.picker = None;
                    match (result, &mut self.view) {
                        (Ok(Some(path)), View::Edit { spec, .. }) => {
                            spec.command = systemd::script_command(&path);
                            self.status = format!("Command set from {path}");
                        }
                        (Err(e), _) => self.status = format!("Error: {e}"),
                        _ => {}
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => self.picker = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }

        if let Some((name, rx)) = &self.running {
            if let Ok(text) = rx.try_recv() {
                let name = name.clone();
                self.running = None;
                self.status = format!("Finished running {name}");
                self.view = View::Logs { name, text };
                self.refresh();
            }
        }

        let p = self.palette;
        egui::Panel::left("nav")
            .exact_size(250.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(p.backdrop()).inner_margin(egui::Margin::same(16)))
            .show(ui, |ui| self.nav_ui(ui));

        if !self.status.is_empty() {
            egui::Panel::bottom("status")
                .frame(egui::Frame::new().fill(p.backdrop()).inner_margin(egui::Margin::symmetric(24, 10)))
                .show(ui, |ui| {
                    ui.label(RichText::new(&self.status).small().color(p.on_surface_variant));
                });
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(p.backdrop()).inner_margin(egui::Margin::symmetric(20, 12)))
            .show(ui, |ui| self.content_ui(ui));

        self.delete_dialog(&ctx);
    }
}

impl App {
    fn nav_ui(&mut self, ui: &mut egui::Ui) {
        let p = self.palette;
        ui.horizontal(|ui| {
            ui.label(icon(icon::SCHEDULE, 24.0).color(p.primary));
            ui.heading("Timer Manager");
        });
        ui.add_space(12.0);
        ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .hint_text("Search timers...")
                .desired_width(f32::INFINITY)
                .margin(egui::Margin::symmetric(10, 7)),
        );
        ui.add_space(10.0);

        let page = self.page();
        let mut go = None;
        for (target, glyph, label) in [
            (Page::Mine, icon::SCHEDULE, "My timers"),
            (Page::All, icon::LIST, "All timers"),
            (Page::New, icon::ADD, "New timer"),
        ] {
            if theme::nav_item(ui, &p, glyph, label, page == target).clicked() {
                go = Some(target);
            }
            ui.add_space(2.0);
        }
        if let Some(target) = go {
            self.go(target);
        }
    }

    fn content_ui(&mut self, ui: &mut egui::Ui) {
        let p = self.palette;
        let ctx = ui.ctx().clone();
        let title = match &self.view {
            View::List if self.show_all => "All timers".to_string(),
            View::List => "My timers".to_string(),
            View::Edit { existing: true, .. } => "Edit timer".to_string(),
            View::Edit { .. } => "New timer".to_string(),
            View::Logs { name, .. } => format!("Output: {name}"),
        };
        let in_list = matches!(self.view, View::List);
        let in_logs = matches!(self.view, View::Logs { .. });

        let mut back = false;
        let mut reload = false;
        ui.horizontal(|ui| {
            if in_logs && theme::icon_button(ui, &p, icon::BACK, "Back").clicked() {
                back = true;
            }
            ui.heading(RichText::new(title).size(20.0));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::icon_button(ui, &p, icon::CLOSE, "Close").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                if in_list && theme::icon_button(ui, &p, icon::REFRESH, "Refresh").clicked() {
                    reload = true;
                }
            });
        });
        ui.add_space(8.0);

        if back {
            self.view = View::List;
            self.refresh();
        }
        if reload {
            systemd::reload();
            self.refresh();
        }

        // Keep the body at a readable width so controls stay near their labels.
        let size = egui::vec2(ui.available_width().min(920.0), ui.available_height());
        ui.allocate_ui_with_layout(size, Layout::top_down(Align::Min), |ui| match &self.view {
            View::List => self.list_ui(ui),
            View::Edit { .. } => {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.edit_ui(ui));
            }
            View::Logs { .. } => self.logs_ui(ui),
        });
    }

    fn list_ui(&mut self, ui: &mut egui::Ui) {
        let p = self.palette;
        let needle = self.search.trim().to_lowercase();
        let mut action: Option<(String, &'static str)> = None;
        let mut shown = 0;

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for t in &self.timers {
                if !needle.is_empty()
                    && !t.name.to_lowercase().contains(&needle)
                    && !t.command.to_lowercase().contains(&needle)
                {
                    continue;
                }
                shown += 1;
                theme::card(ui, &p, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(icon(icon::SCHEDULE, 20.0).color(p.primary));
                        ui.heading(&t.name);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if theme::toggle(ui, &p, t.enabled()).clicked() {
                                let what = if t.enabled() { "disable" } else { "enable" };
                                action = Some((t.name.clone(), what));
                            }
                            let (label, color) = if t.enabled() {
                                ("Enabled", p.success)
                            } else {
                                ("Disabled", p.on_surface_variant)
                            };
                            ui.label(RichText::new(label).small().color(color));
                        });
                    });
                    ui.add_space(2.0);
                    ui.label(RichText::new(&t.summary).color(p.on_surface));
                    let runs = if t.command.is_empty() { "(unknown)" } else { &t.command };
                    ui.horizontal(|ui| {
                        ui.label(icon(icon::TERMINAL, 16.0).color(p.on_surface_variant));
                        ui.label(RichText::new(runs).monospace().color(p.on_surface_variant));
                    });
                    ui.label(
                        RichText::new(format!(
                            "Next: {}     Last: {}",
                            format_time(t.next),
                            format_time(t.last)
                        ))
                        .small()
                        .color(p.on_surface_variant),
                    );
                    theme::divider(ui);
                    ui.horizontal_wrapped(|ui| {
                        let n = || t.name.clone();
                        if theme::action_button(ui, &p, icon::PLAY, "Run now", false).clicked() {
                            action = Some((n(), "run"));
                        }
                        if theme::action_button(ui, &p, icon::ARTICLE, "Logs", false).clicked() {
                            action = Some((n(), "logs"));
                        }
                        if theme::action_button(ui, &p, icon::EDIT, "Edit", false).clicked() {
                            action = Some((n(), "edit"));
                        }
                        if t.managed && theme::action_button(ui, &p, icon::DELETE, "Delete", true).clicked() {
                            action = Some((n(), "delete"));
                        }
                    });
                });
                ui.add_space(8.0);
            }
            if shown == 0 {
                ui.label(RichText::new("No timers found.").color(p.on_surface_variant));
            }
        });

        let Some((name, what)) = action else { return };
        match what {
            "run" if self.running.is_some() => {
                self.status = "A job is already running".into();
            }
            "run" => {
                let (tx, rx) = mpsc::channel();
                let (job, ctx) = (name.clone(), ui.ctx().clone());
                std::thread::spawn(move || {
                    let _ = tx.send(systemd::run_now(&job));
                    ctx.request_repaint();
                });
                self.status = format!("Running {name}...");
                self.running = Some((name, rx));
            }
            "enable" | "disable" => {
                let r = systemd::set_enabled(&name, what == "enable");
                self.report(r);
            }
            "logs" => {
                let text = systemd::logs(&name);
                self.view = View::Logs { name, text };
            }
            "edit" => match Spec::load(&name) {
                Some(spec) => self.view = View::Edit { spec, existing: true },
                None => {
                    self.status = match systemd::open_in_editor(&name) {
                        Ok(msg) => msg,
                        Err(e) => format!("Error: {e}"),
                    }
                }
            },
            "delete" => self.confirm_delete = Some(name),
            _ => {}
        }
    }

    fn logs_ui(&mut self, ui: &mut egui::Ui) {
        let p = self.palette;
        let View::Logs { text, .. } = &self.view else { return };
        let mut text = text.clone();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            theme::card(ui, &p, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut text)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .frame(egui::Frame::new()),
                );
            });
        });
    }

    fn edit_ui(&mut self, ui: &mut egui::Ui) {
        let p = self.palette;
        let View::Edit { spec, existing } = &mut self.view else { return };
        let existing = *existing;
        let mut save = false;
        let mut cancel = false;
        let mut browse = false;
        let mut preset: Option<usize> = None;

        // Basics
        theme::card(ui, &p, |ui| {
            theme::section_header(ui, &p, icon::TUNE, "Basics");
            if !existing {
                theme::setting_row(
                    ui,
                    &p,
                    "Start from",
                    "Fill in a typical schedule and options. You can change anything afterwards.",
                    |ui| {
                        egui::ComboBox::from_id_salt("preset")
                            .selected_text("Pick a preset (optional)")
                            .width(280.0)
                            .show_ui(ui, |ui| {
                                for (i, label) in systemd::PRESETS.iter().enumerate() {
                                    if ui.selectable_label(false, *label).clicked() {
                                        preset = Some(i);
                                    }
                                }
                            });
                    },
                );
                theme::divider(ui);
            }
            theme::setting_row(ui, &p, "Name", "Letters, numbers, - and _. It can't be changed later.", |ui| {
                ui.add_enabled(
                    !existing,
                    egui::TextEdit::singleline(&mut spec.name)
                        .desired_width(280.0)
                        .hint_text("e.g. backup-notes")
                        .margin(egui::Margin::symmetric(10, 8)),
                );
            });
            theme::divider(ui);
            theme::setting_row(ui, &p, "Description", "A note about what this timer is for. Optional.", |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut spec.description)
                        .desired_width(280.0)
                        .margin(egui::Margin::symmetric(10, 8)),
                );
            });
            theme::divider(ui);
            theme::setting_row(ui, &p, "Command", "What to run: a shell command, or pick a script.", |ui| {
                if theme::action_button(ui, &p, icon::FOLDER, "Browse", false).clicked() {
                    browse = true;
                }
                ui.add(
                    egui::TextEdit::singleline(&mut spec.command)
                        .desired_width(200.0)
                        .hint_text("command or script")
                        .margin(egui::Margin::symmetric(10, 8)),
                );
            });
        });
        ui.add_space(8.0);

        // Schedule
        theme::card(ui, &p, |ui| {
            theme::section_header(ui, &p, icon::SCHEDULE, "Schedule");
            theme::setting_row(ui, &p, "Repeat", "How often it runs.", |ui| {
                egui::ComboBox::from_id_salt("kind")
                    .selected_text(spec.kind.label())
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        for k in Kind::ALL {
                            ui.selectable_value(&mut spec.kind, k, k.label());
                        }
                    });
            });
            theme::divider(ui);
            theme::setting_row(ui, &p, "When", "The time or interval for the choice above.", |ui| {
                // This row lays out right-to-left, so widgets are added last-to-first.
                match spec.kind {
                    Kind::Minutes => {
                        ui.label("minutes");
                        ui.add(egui::DragValue::new(&mut spec.interval).range(1..=59));
                        ui.label("every");
                    }
                    Kind::Hours => {
                        ui.add(egui::DragValue::new(&mut spec.minute).range(0..=59));
                        ui.label("hours, at minute");
                        ui.add(egui::DragValue::new(&mut spec.interval).range(1..=23));
                        ui.label("every");
                    }
                    Kind::Daily | Kind::Weekly | Kind::Monthly => {
                        ui.add(
                            egui::DragValue::new(&mut spec.minute)
                                .range(0..=59)
                                .custom_formatter(|n, _| format!("{:02}", n as u32)),
                        );
                        ui.label(":");
                        ui.add(
                            egui::DragValue::new(&mut spec.hour)
                                .range(0..=23)
                                .custom_formatter(|n, _| format!("{:02}", n as u32)),
                        );
                        ui.label("at");
                        if spec.kind == Kind::Monthly {
                            ui.add(egui::DragValue::new(&mut spec.day).range(1..=31));
                            ui.label("day");
                        }
                        if spec.kind == Kind::Weekly {
                            egui::ComboBox::from_id_salt("weekday")
                                .selected_text(WEEKDAYS[spec.weekday % 7])
                                .show_ui(ui, |ui| {
                                    for (i, d) in WEEKDAYS.iter().enumerate() {
                                        ui.selectable_value(&mut spec.weekday, i, *d);
                                    }
                                });
                        }
                    }
                    Kind::Custom => {
                        ui.add(
                            egui::TextEdit::singleline(&mut spec.custom)
                                .desired_width(240.0)
                                .hint_text("e.g. Mon..Fri *-*-* 08:30:00")
                                .margin(egui::Margin::symmetric(10, 8)),
                        );
                    }
                }
            });
            theme::divider(ui);
            theme::setting_row(
                ui,
                &p,
                "Catch up after being off",
                "If the PC was off at the scheduled time, run the job when it next starts.",
                |ui| {
                    if theme::toggle(ui, &p, spec.persistent).clicked() {
                        spec.persistent = !spec.persistent;
                    }
                },
            );

            let calendar = spec.on_calendar();
            if self.preview.0 != calendar {
                let result = if calendar.is_empty() {
                    Ok(String::new())
                } else {
                    systemd::preview_calendar(&calendar)
                };
                self.preview = (calendar.clone(), result);
            }
            theme::divider(ui);
            ui.label(RichText::new(spec.describe()).strong().color(p.on_surface));
            match &self.preview.1 {
                Ok(next) => {
                    ui.label(RichText::new(next).small().color(p.on_surface_variant));
                }
                Err(e) => {
                    ui.colored_label(p.error, e);
                }
            }
            ui.label(
                RichText::new(format!("OnCalendar={calendar}"))
                    .small()
                    .weak()
                    .color(p.on_surface_variant),
            );
            for note in systemd::warnings(spec) {
                ui.colored_label(egui::Color32::from_rgb(230, 180, 60), note);
            }
        });
        ui.add_space(8.0);

        // Service options
        collapsible_card(ui, &p, "Service options", existing, |ui| {
            theme::setting_row(ui, &p, "Type", "Run once and finish suits scripts. Keep running suits long-lived programs.", |ui| {
                egui::ComboBox::from_id_salt("service_type")
                    .selected_text(spec.service_type.label())
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        for t in ServiceType::ALL {
                            ui.selectable_value(&mut spec.service_type, t, t.label());
                        }
                    });
            });
            theme::divider(ui);
            theme::setting_row(ui, &p, "Working directory", "The folder the command starts in. Optional.", |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut spec.workdir)
                        .desired_width(280.0)
                        .hint_text("e.g. ~/Projects")
                        .margin(egui::Margin::symmetric(10, 8)),
                );
            });
            theme::divider(ui);
            theme::setting_row(ui, &p, "Environment", "Extra variables for the command, one KEY=value per line.", |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut spec.env)
                        .desired_rows(2)
                        .desired_width(280.0)
                        .margin(egui::Margin::symmetric(10, 8)),
                );
            });
            theme::divider(ui);
            theme::setting_row(ui, &p, "Timeout", "Stop the job if it runs longer than this. 0 means no limit.", |ui| {
                ui.add(egui::DragValue::new(&mut spec.timeout).range(0..=86400).suffix(" s"));
            });
            theme::divider(ui);
            theme::setting_row(ui, &p, "Retry on failure", "Run the job again after a delay if it fails.", |ui| {
                if spec.restart != Restart::No {
                    ui.add(egui::DragValue::new(&mut spec.restart_delay).range(1..=3600).suffix(" s"));
                    ui.label("after");
                }
                egui::ComboBox::from_id_salt("restart")
                    .selected_text(spec.restart.label())
                    .width(120.0)
                    .show_ui(ui, |ui| {
                        for r in Restart::ALL {
                            ui.selectable_value(&mut spec.restart, r, r.label());
                        }
                    });
            });
            theme::divider(ui);
            theme::setting_row(ui, &p, "Low priority", "Lets other programs use the CPU and disk first.", |ui| {
                if theme::toggle(ui, &p, spec.low_priority).clicked() {
                    spec.low_priority = !spec.low_priority;
                }
            });
        });
        ui.add_space(8.0);

        // Timer options
        collapsible_card(ui, &p, "Timer options", existing, |ui| {
            theme::setting_row(ui, &p, "Random delay", "Spread start times: wait up to this long after the scheduled time. 0 means none.", |ui| {
                ui.add(egui::DragValue::new(&mut spec.random_delay).range(0..=86400).suffix(" s"));
            });
            theme::divider(ui);
            theme::setting_row(ui, &p, "Also run after boot", "Run this many minutes after the PC starts. 0 means off.", |ui| {
                ui.add(egui::DragValue::new(&mut spec.on_boot).range(0..=1440).suffix(" min"));
            });
        });
        ui.add_space(8.0);

        // Extra settings
        collapsible_card(ui, &p, "Extra settings (advanced)", existing, |ui| {
            ui.label(
                RichText::new("Raw systemd lines, one per line, added after the options above.")
                    .small()
                    .color(p.on_surface_variant),
            );
            for (title, hint, text) in [
                ("[Unit] (applies to the service)", "e.g. Documentation=https://...", &mut spec.extra_unit),
                ("[Service]", "e.g. ProtectHome=read-only", &mut spec.extra_service),
                ("[Timer]", "e.g. AccuracySec=1min", &mut spec.extra_timer),
            ] {
                ui.add_space(6.0);
                ui.label(RichText::new(title).color(p.on_surface));
                ui.add(
                    egui::TextEdit::multiline(text)
                        .font(egui::TextStyle::Monospace)
                        .desired_rows(2)
                        .desired_width(f32::INFINITY)
                        .hint_text(hint)
                        .margin(egui::Margin::symmetric(10, 8)),
                );
            }
        });
        ui.add_space(16.0);

        ui.horizontal(|ui| {
            if theme::primary_button(ui, &p, icon::CHECK, "Save").clicked() {
                save = true;
            }
            if theme::action_button(ui, &p, icon::CLOSE, "Cancel", false).clicked() {
                cancel = true;
            }
        });
        ui.add_space(24.0);

        if let Some(i) = preset {
            if let View::Edit { spec, .. } = &mut self.view {
                systemd::apply_preset(spec, i);
                self.status = format!("Applied preset: {}", systemd::PRESETS[i]);
            }
        }

        if browse && self.picker.is_none() {
            let (tx, rx) = mpsc::channel();
            let ctx = ui.ctx().clone();
            std::thread::spawn(move || {
                let _ = tx.send(pick_file());
                ctx.request_repaint();
            });
            self.picker = Some(rx);
        }

        if save {
            let (spec_copy, existing) = match &self.view {
                View::Edit { spec, existing } => (spec.clone(), *existing),
                _ => return,
            };
            match systemd::save(&spec_copy, existing) {
                Ok(msg) => {
                    self.view = View::List;
                    self.report(Ok(msg));
                }
                Err(e) => self.status = format!("Error: {e}"),
            }
        } else if cancel {
            self.view = View::List;
        }
    }

    fn delete_dialog(&mut self, ctx: &egui::Context) {
        let Some(name) = self.confirm_delete.clone() else { return };
        let p = self.palette;
        let mut close = false;
        egui::Window::new("Delete timer?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(format!("Delete {name} and its service? This cannot be undone."));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if theme::action_button(ui, &p, icon::DELETE, "Delete", true).clicked() {
                        let r = systemd::delete(&name);
                        self.report(r);
                        close = true;
                    }
                    if theme::action_button(ui, &p, icon::CLOSE, "Cancel", false).clicked() {
                        close = true;
                    }
                });
            });
        if close {
            self.confirm_delete = None;
        }
    }
}

/// A card whose body folds away under its title.
fn collapsible_card(ui: &mut egui::Ui, p: &Palette, title: &str, open: bool, body: impl FnOnce(&mut egui::Ui)) {
    theme::card(ui, p, |ui| {
        egui::CollapsingHeader::new(RichText::new(title).text_style(egui::TextStyle::Heading))
            .default_open(open)
            .show(ui, |ui| {
                ui.add_space(4.0);
                body(ui);
            });
    });
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1040.0, 700.0])
            .with_min_inner_size([760.0, 520.0])
            .with_decorations(false)
            .with_transparent(true)
            .with_app_id("timer-manager"),
        ..Default::default()
    };
    eframe::run_native(
        "Timer Manager",
        options,
        Box::new(|cc| {
            theme::install_fonts(&cc.egui_ctx);
            theme::apply_text_styles(&cc.egui_ctx);
            let palette = theme::load_palette();
            theme::apply_palette(&cc.egui_ctx, &palette);
            cc.egui_ctx.set_zoom_factor(1.1);
            Ok(Box::new(App::new(palette)))
        }),
    )
}
