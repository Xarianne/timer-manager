//! The New timer / Edit timer form.

use std::{cell::RefCell, rc::Rc};

use adw::prelude::*;
use gtk::{gio, glib};

use crate::systemd::{self, Kind, Restart, ServiceType, Spec, WEEKDAYS};

/// Changes the spec and refreshes the schedule summary.
type Set = Rc<dyn Fn(Box<dyn FnOnce(&mut Spec)>)>;

pub struct Form {
    pub page: adw::PreferencesPage,
    /// Kept up to date by the widgets.
    pub spec: Rc<RefCell<Spec>>,
}

/// `preset` is the preset the "Start from" row shows (0 is none). Picking one calls
/// `on_preset` with the filled-in spec, so the caller can rebuild the form from it.
pub fn build(spec: Spec, existing: bool, preset: u32, on_preset: impl Fn(Spec, u32) + 'static) -> Form {
    let spec = Rc::new(RefCell::new(spec));
    let page = adw::PreferencesPage::new();

    // The plain-English summary under the schedule, refreshed on every change.
    let describe = label(&["heading"]);
    let next = label(&["caption"]);
    let calendar = label(&["caption", "dim-label"]);
    let warnings = label(&["caption", "warning"]);
    let summary = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();
    for l in [&describe, &next, &calendar, &warnings] {
        summary.append(l);
    }
    let preview: RefCell<(Option<String>, Result<String, String>)> = RefCell::new((None, Ok(String::new())));
    let update = {
        let spec = spec.clone();
        move || {
            let spec = spec.borrow();
            describe.set_label(&spec.describe());
            let cal = spec.on_calendar();
            let mut preview = preview.borrow_mut();
            if preview.0.as_deref() != Some(cal.as_str()) {
                let result = if cal.is_empty() { Ok(String::new()) } else { systemd::preview_calendar(&cal) };
                *preview = (Some(cal.clone()), result);
            }
            let (text, error) = match &preview.1 {
                Ok(text) => (text.as_str(), false),
                Err(e) => (e.as_str(), true),
            };
            next.set_label(text);
            next.set_visible(!text.is_empty());
            if error {
                next.add_css_class("error");
                next.remove_css_class("dim-label");
            } else {
                next.remove_css_class("error");
                next.add_css_class("dim-label");
            }
            calendar.set_label(&format!("OnCalendar={cal}"));
            let notes = systemd::warnings(&spec);
            warnings.set_label(&notes.join("\n"));
            warnings.set_visible(!notes.is_empty());
        }
    };
    update();
    let set: Set = {
        let spec = spec.clone();
        Rc::new(move |change| {
            change(&mut spec.borrow_mut());
            update();
        })
    };

    // Basics
    let basics = adw::PreferencesGroup::builder().title("Basics").build();
    if !existing {
        let mut labels = vec!["None"];
        labels.extend(systemd::PRESETS);
        let presets = combo_row(
            "Start from",
            "Fill in a typical schedule and options. You can change anything afterwards.",
            &labels,
            preset,
        );
        let current = spec.clone();
        presets.connect_selected_notify(move |row| {
            let index = row.selected();
            if index == 0 || index == gtk::INVALID_LIST_POSITION {
                return;
            }
            let mut filled = current.borrow().clone();
            systemd::apply_preset(&mut filled, index as usize - 1);
            on_preset(filled, index);
        });
        basics.add(&presets);
    }
    let s = spec.borrow();
    let name = entry_row(&basics, "Name", "Letters, numbers, - and _. It can't be changed later.", &s.name, "e.g. backup-notes", &set, |sp, v| sp.name = v);
    name.set_sensitive(!existing);
    entry_row(&basics, "Description", "A note about what this timer is for. Optional.", &s.description, "", &set, |sp, v| sp.description = v);

    // Commands are long, so the field gets its own full-width line under the title.
    let command_row = adw::ActionRow::builder()
        .title("Command")
        .subtitle("What to run: a shell command, or pick a script.")
        .build();
    let browse = gtk::Button::builder().label("Browse…").valign(gtk::Align::Center).build();
    command_row.add_suffix(&browse);
    basics.add(&command_row);
    let command = gtk::Entry::builder()
        .text(&s.command)
        .placeholder_text("command or script")
        .hexpand(true)
        .build();
    let setter = set.clone();
    command.connect_changed(move |e| {
        let v = e.text().to_string();
        setter(Box::new(move |sp| sp.command = v));
    });
    basics.add(&padded_row(&command));
    let target = command.clone();
    browse.connect_clicked(move |button| {
        let window = button.root().and_downcast::<gtk::Window>();
        let target = target.clone();
        gtk::FileDialog::builder()
            .title("Pick a script")
            .modal(true)
            .build()
            .open(window.as_ref(), gio::Cancellable::NONE, move |result| {
                if let Some(path) = result.ok().and_then(|f| f.path()) {
                    target.set_text(&systemd::script_command(&path.to_string_lossy()));
                }
            });
    });
    page.add(&basics);

    // Schedule
    let schedule = adw::PreferencesGroup::builder().title("Schedule").build();
    let kinds: Vec<&str> = Kind::ALL.iter().map(|k| k.label()).collect();
    let kind_index = Kind::ALL.iter().position(|k| *k == s.kind).unwrap_or(0) as u32;
    let repeat = combo_row("Repeat", "How often it runs.", &kinds, kind_index);
    schedule.add(&repeat);
    let when_row = adw::ActionRow::builder()
        .title("When")
        .subtitle("The time or interval for the choice above.")
        .build();
    let when = gtk::Box::builder().spacing(6).valign(gtk::Align::Center).build();
    when_row.add_suffix(&when);
    schedule.add(&when_row);
    fill_when(&when, &s, &set);
    let (setter, current, box_) = (set.clone(), spec.clone(), when.clone());
    repeat.connect_selected_notify(move |row| {
        let kind = Kind::ALL[(row.selected() as usize).min(Kind::ALL.len() - 1)];
        setter(Box::new(move |sp| sp.kind = kind));
        fill_when(&box_, &current.borrow(), &setter);
    });
    switch_row(
        &schedule,
        "Catch up after being off",
        "If the PC was off at the scheduled time, run the job when it next starts.",
        s.persistent,
        &set,
        |sp, on| sp.persistent = on,
    );
    schedule.add(&gtk::ListBoxRow::builder().activatable(false).child(&summary).build());
    page.add(&schedule);

    // The rarer options fold away; they start open when editing.
    let options = adw::PreferencesGroup::builder().title("More options").build();

    let service = expander(
        "Service options",
        "Type, working directory, environment, timeout, retries, priority",
        existing,
    );
    let types: Vec<&str> = ServiceType::ALL.iter().map(|t| t.label()).collect();
    let type_index = ServiceType::ALL.iter().position(|t| *t == s.service_type).unwrap_or(0) as u32;
    let service_type = combo_row(
        "Type",
        "Run once and finish suits scripts. Keep running suits long-lived programs.",
        &types,
        type_index,
    );
    let setter = set.clone();
    service_type.connect_selected_notify(move |row| {
        let t = ServiceType::ALL[(row.selected() as usize).min(ServiceType::ALL.len() - 1)];
        setter(Box::new(move |sp| sp.service_type = t));
    });
    service.add_row(&service_type);
    let workdir = adw::ActionRow::builder()
        .title("Working directory")
        .subtitle("The folder the command starts in. Optional.")
        .build();
    workdir.add_suffix(&entry(&s.workdir, "e.g. ~/Projects", &set, |sp, v| sp.workdir = v));
    service.add_row(&workdir);
    service.add_row(
        &adw::ActionRow::builder()
            .title("Environment")
            .subtitle("Extra variables for the command, one KEY=value per line.")
            .build(),
    );
    service.add_row(&text_row(&s.env, &set, |sp, v| sp.env = v));
    spin_row(
        &service,
        "Timeout",
        "Stop the job if it runs longer than this many seconds. 0 means no limit.",
        (0, 86400),
        s.timeout,
        &set,
        |sp, v| sp.timeout = v,
    );
    let restarts: Vec<&str> = Restart::ALL.iter().map(|r| r.label()).collect();
    let restart_index = Restart::ALL.iter().position(|r| *r == s.restart).unwrap_or(0) as u32;
    let restart = combo_row("Retry on failure", "Run the job again after a delay if it fails.", &restarts, restart_index);
    service.add_row(&restart);
    let delay = spin_row(
        &service,
        "Retry after",
        "Seconds to wait before running it again.",
        (1, 3600),
        s.restart_delay,
        &set,
        |sp, v| sp.restart_delay = v,
    );
    delay.set_visible(s.restart != Restart::No);
    let setter = set.clone();
    restart.connect_selected_notify(move |row| {
        let r = Restart::ALL[(row.selected() as usize).min(Restart::ALL.len() - 1)];
        delay.set_visible(r != Restart::No);
        setter(Box::new(move |sp| sp.restart = r));
    });
    let low = adw::SwitchRow::builder()
        .title("Low priority")
        .subtitle("Lets other programs use the CPU and disk first.")
        .active(s.low_priority)
        .build();
    let setter = set.clone();
    low.connect_active_notify(move |row| {
        let on = row.is_active();
        setter(Box::new(move |sp| sp.low_priority = on));
    });
    service.add_row(&low);
    options.add(&service);

    let timer = expander("Timer options", "Random delay, also run after boot", existing);
    spin_row(
        &timer,
        "Random delay",
        "Spread start times: wait up to this many seconds after the scheduled time. 0 means none.",
        (0, 86400),
        s.random_delay,
        &set,
        |sp, v| sp.random_delay = v,
    );
    spin_row(
        &timer,
        "Also run after boot",
        "Run this many minutes after the PC starts. 0 means off.",
        (0, 1440),
        s.on_boot,
        &set,
        |sp, v| sp.on_boot = v,
    );
    options.add(&timer);

    let extra = expander(
        "Extra settings (advanced)",
        "Raw systemd lines, one per line, added after the options above",
        existing,
    );
    let sections: [(&str, &str, &str, fn(&mut Spec, String)); 3] = [
        ("[Unit] (applies to the service)", "e.g. Documentation=https://...", &s.extra_unit, |sp, v| sp.extra_unit = v),
        ("[Service]", "e.g. ProtectHome=read-only", &s.extra_service, |sp, v| sp.extra_service = v),
        ("[Timer]", "e.g. AccuracySec=1min", &s.extra_timer, |sp, v| sp.extra_timer = v),
    ];
    for (title, hint, text, apply) in sections {
        extra.add_row(&adw::ActionRow::builder().title(title).subtitle(hint).build());
        extra.add_row(&text_row(text, &set, apply));
    }
    options.add(&extra);
    page.add(&options);

    drop(s);
    Form { page, spec }
}

fn label(classes: &[&str]) -> gtk::Label {
    gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .selectable(true)
        .css_classes(classes.to_vec())
        .build()
}

fn combo_row(title: &str, subtitle: &str, items: &[&str], selected: u32) -> adw::ComboRow {
    adw::ComboRow::builder()
        .title(title)
        .subtitle(subtitle)
        .model(&gtk::StringList::new(items))
        .selected(selected)
        .build()
}

fn expander(title: &str, subtitle: &str, open: bool) -> adw::ExpanderRow {
    adw::ExpanderRow::builder().title(title).subtitle(subtitle).expanded(open).build()
}

/// A row holding one widget across its full width.
fn padded_row(child: &impl IsA<gtk::Widget>) -> gtk::ListBoxRow {
    let holder = gtk::Box::builder()
        .margin_start(12)
        .margin_end(12)
        .margin_bottom(12)
        .build();
    holder.append(child);
    gtk::ListBoxRow::builder().activatable(false).child(&holder).build()
}

fn entry(text: &str, hint: &str, set: &Set, apply: fn(&mut Spec, String)) -> gtk::Entry {
    let entry = gtk::Entry::builder()
        .text(text)
        .placeholder_text(hint)
        .valign(gtk::Align::Center)
        .width_chars(26)
        .build();
    let set = set.clone();
    entry.connect_changed(move |e| {
        let v = e.text().to_string();
        set(Box::new(move |sp| apply(sp, v)));
    });
    entry
}

/// A title and description on the left, a text field on the right.
fn entry_row(
    group: &adw::PreferencesGroup,
    title: &str,
    subtitle: &str,
    text: &str,
    hint: &str,
    set: &Set,
    apply: fn(&mut Spec, String),
) -> gtk::Entry {
    let row = adw::ActionRow::builder().title(title).subtitle(subtitle).build();
    let field = entry(text, hint, set, apply);
    row.add_suffix(&field);
    group.add(&row);
    field
}

fn switch_row(
    group: &adw::PreferencesGroup,
    title: &str,
    subtitle: &str,
    on: bool,
    set: &Set,
    apply: fn(&mut Spec, bool),
) {
    let row = adw::SwitchRow::builder().title(title).subtitle(subtitle).active(on).build();
    let set = set.clone();
    row.connect_active_notify(move |row| {
        let on = row.is_active();
        set(Box::new(move |sp| apply(sp, on)));
    });
    group.add(&row);
}

fn spin_row(
    parent: &adw::ExpanderRow,
    title: &str,
    subtitle: &str,
    (min, max): (u32, u32),
    value: u32,
    set: &Set,
    apply: fn(&mut Spec, u32),
) -> adw::SpinRow {
    let row = adw::SpinRow::with_range(min as f64, max as f64, 1.0);
    row.set_title(title);
    row.set_subtitle(subtitle);
    row.set_value(value as f64);
    let set = set.clone();
    row.connect_notify_local(Some("value"), move |row, _| {
        let v = row.value() as u32;
        set(Box::new(move |sp| apply(sp, v)));
    });
    parent.add_row(&row);
    row
}

/// A multi-line monospace field across the row.
fn text_row(text: &str, set: &Set, apply: fn(&mut Spec, String)) -> gtk::ListBoxRow {
    let view = gtk::TextView::builder()
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .top_margin(8)
        .bottom_margin(8)
        .left_margin(8)
        .right_margin(8)
        .build();
    view.buffer().set_text(text);
    let set = set.clone();
    view.buffer().connect_changed(move |b| {
        let v = b.text(&b.start_iter(), &b.end_iter(), false).to_string();
        set(Box::new(move |sp| apply(sp, v)));
    });
    let frame = gtk::Frame::builder().child(&view).hexpand(true).height_request(64).build();
    padded_row(&frame)
}

/// A spin button for one number; `pad` shows it with two digits, for times.
fn spin(min: u32, max: u32, value: u32, pad: bool, set: &Set, apply: fn(&mut Spec, u32)) -> gtk::SpinButton {
    let spin = gtk::SpinButton::with_range(min as f64, max as f64, 1.0);
    spin.set_value(value as f64);
    if pad {
        spin.connect_output(|s| {
            s.set_text(&format!("{:02}", s.value() as u32));
            glib::Propagation::Stop
        });
    }
    let set = set.clone();
    spin.connect_value_changed(move |s| {
        let v = s.value() as u32;
        set(Box::new(move |sp| apply(sp, v)));
    });
    spin
}

/// Fills the "When" row with the fields the chosen schedule kind needs.
fn fill_when(when: &gtk::Box, spec: &Spec, set: &Set) {
    while let Some(child) = when.first_child() {
        when.remove(&child);
    }
    let text = |s: &str| gtk::Label::new(Some(s));
    match spec.kind {
        Kind::Minutes => {
            when.append(&text("every"));
            when.append(&spin(1, 59, spec.interval, false, set, |sp, v| sp.interval = v));
            when.append(&text("minutes"));
        }
        Kind::Hours => {
            when.append(&text("every"));
            when.append(&spin(1, 23, spec.interval, false, set, |sp, v| sp.interval = v));
            when.append(&text("hours, at minute"));
            when.append(&spin(0, 59, spec.minute, true, set, |sp, v| sp.minute = v));
        }
        Kind::Daily | Kind::Weekly | Kind::Monthly => {
            if spec.kind == Kind::Weekly {
                let days = gtk::DropDown::from_strings(&WEEKDAYS);
                days.set_selected((spec.weekday % 7) as u32);
                let set = set.clone();
                days.connect_selected_notify(move |d| {
                    let day = d.selected() as usize % 7;
                    set(Box::new(move |sp| sp.weekday = day));
                });
                when.append(&days);
            }
            if spec.kind == Kind::Monthly {
                when.append(&text("day"));
                when.append(&spin(1, 31, spec.day, false, set, |sp, v| sp.day = v));
            }
            when.append(&text("at"));
            when.append(&spin(0, 23, spec.hour, true, set, |sp, v| sp.hour = v));
            when.append(&text(":"));
            when.append(&spin(0, 59, spec.minute, true, set, |sp, v| sp.minute = v));
        }
        Kind::Custom => {
            when.append(&entry(&spec.custom, "e.g. Mon..Fri *-*-* 08:30:00", set, |sp, v| sp.custom = v));
        }
    }
}
