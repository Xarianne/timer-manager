mod form;
mod systemd;

use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    rc::Rc,
};

use adw::prelude::*;
use chrono::{Local, TimeZone};
use gtk::{gio, glib};
use systemd::{Spec, TimerInfo};

const APP_ID: &str = "io.github.xarianne.TimerManager";

/// Sidebar rows, in order.
const MINE: i32 = 0;
const ALL: i32 = 1;
const NEW: i32 = 2;

struct State {
    window: adw::ApplicationWindow,
    split: adw::NavigationSplitView,
    page: adw::NavigationPage,
    stack: gtk::Stack,
    nav: gtk::ListBox,
    list: gtk::ListBox,
    toasts: adw::ToastOverlay,
    reload: gtk::Button,
    save: gtk::Button,
    cancel: gtk::Button,
    show_all: Cell<bool>,
    search: RefCell<String>,
    /// Rows the user opened, so a refresh keeps them open.
    expanded: RefCell<HashSet<String>>,
    /// The spec behind the open form, and whether it edits an existing timer.
    form: RefCell<Option<(Rc<RefCell<Spec>>, bool)>>,
    running: Cell<bool>,
}

fn main() -> glib::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let start = if args.iter().any(|a| a == "--new") {
        NEW
    } else if args.iter().any(|a| a == "--all") {
        ALL
    } else {
        MINE
    };
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(move |app| build_window(app, start));
    // GApplication rejects options it doesn't know, so it only gets the program name.
    app.run_with_args(&args[..1])
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

fn build_window(app: &adw::Application, start: i32) {
    if let Some(window) = app.active_window() {
        window.present();
        return;
    }

    // Sidebar: search and the pages.
    let search = gtk::SearchEntry::builder()
        .placeholder_text("Search timers")
        .margin_start(12)
        .margin_end(12)
        .build();
    let nav = gtk::ListBox::builder().css_classes(["navigation-sidebar"]).build();
    for label in ["My timers", "All timers", "New timer"] {
        nav.append(&gtk::Label::builder().label(label).xalign(0.0).build());
    }
    let sidebar_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();
    sidebar_box.append(&search);
    sidebar_box.append(&nav);
    let sidebar_view = adw::ToolbarView::new();
    sidebar_view.add_top_bar(&adw::HeaderBar::new());
    sidebar_view.set_content(Some(&sidebar_box));
    let sidebar = adw::NavigationPage::builder()
        .title("Timer Manager")
        .child(&sidebar_view)
        .build();

    // Content: the timer list, or the form.
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .valign(gtk::Align::Start)
        .build();
    let clamp = adw::Clamp::builder()
        .maximum_size(860)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(12)
        .margin_end(12)
        .child(&list)
        .build();
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&clamp)
        .build();
    let stack = gtk::Stack::new();
    stack.add_named(&scroll, Some("list"));
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&stack));

    let header = adw::HeaderBar::new();
    let reload = gtk::Button::builder()
        .icon_name("view-refresh-symbolic")
        .tooltip_text("Refresh")
        .build();
    let save = gtk::Button::builder()
        .label("Save")
        .css_classes(["suggested-action"])
        .visible(false)
        .build();
    let cancel = gtk::Button::builder().label("Cancel").visible(false).build();
    header.pack_start(&cancel);
    header.pack_end(&reload);
    header.pack_end(&save);
    let content_view = adw::ToolbarView::new();
    content_view.add_top_bar(&header);
    content_view.set_content(Some(&toasts));
    let page = adw::NavigationPage::builder()
        .title("My timers")
        .child(&content_view)
        .build();

    let split = adw::NavigationSplitView::builder()
        .sidebar(&sidebar)
        .content(&page)
        .min_sidebar_width(220.0)
        .build();
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Timer Manager")
        .default_width(1000)
        .default_height(720)
        .width_request(360)
        .height_request(300)
        .content(&split)
        .build();
    let narrow = adw::Breakpoint::new(adw::BreakpointCondition::parse("max-width: 600sp").unwrap());
    narrow.add_setter(&split, "collapsed", Some(&true.to_value()));
    window.add_breakpoint(narrow);

    let state = Rc::new(State {
        window: window.clone(),
        split,
        page,
        stack,
        nav: nav.clone(),
        list,
        toasts,
        reload: reload.clone(),
        save: save.clone(),
        cancel: cancel.clone(),
        show_all: Cell::new(start == ALL),
        search: RefCell::new(String::new()),
        expanded: RefCell::new(HashSet::new()),
        form: RefCell::new(None),
        running: Cell::new(false),
    });

    // Activation, not selection: GTK also selects rows on its own, e.g. when the list gets focus.
    let s = state.clone();
    nav.connect_row_activated(move |_, row| {
        match row.index() {
            NEW => {
                // Clicking it again while filling in a new timer keeps what was typed.
                if !matches!(&*s.form.borrow(), Some((_, false))) {
                    open_form(&s, Spec::default(), false, 0);
                }
            }
            index => show_list(&s, index == ALL),
        }
        s.split.set_show_content(true);
    });
    let s = state.clone();
    search.connect_search_changed(move |entry| {
        *s.search.borrow_mut() = entry.text().to_string();
        if s.form.borrow().is_some() {
            show_list(&s, s.show_all.get());
        } else {
            refresh(&s);
        }
    });
    let s = state.clone();
    reload.connect_clicked(move |_| {
        systemd::reload();
        refresh(&s);
    });
    let s = state.clone();
    cancel.connect_clicked(move |_| show_list(&s, s.show_all.get()));
    let s = state.clone();
    save.connect_clicked(move |_| {
        let Some((spec, existing)) = s.form.borrow().clone() else { return };
        let spec = spec.borrow().clone();
        match systemd::save(&spec, existing) {
            Ok(msg) => {
                toast(&s, &msg);
                show_list(&s, s.show_all.get());
            }
            Err(e) => alert(&s, "Couldn't save the timer", &e),
        }
    });
    // Coming back to the window, e.g. from a text editor, shows any changes.
    let s = state.clone();
    window.connect_is_active_notify(move |w| {
        if w.is_active() && s.form.borrow().is_none() {
            refresh(&s);
        }
    });

    match start {
        NEW => open_form(&state, Spec::default(), false, 0),
        _ => show_list(&state, start == ALL),
    }
    window.present();
}

fn toast(state: &State, text: &str) {
    state.toasts.add_toast(adw::Toast::new(text));
}

fn alert(state: &State, heading: &str, body: &str) {
    let dialog = adw::AlertDialog::new(Some(heading), Some(body));
    dialog.add_response("close", "Close");
    dialog.present(Some(&state.window));
}

/// Shows the outcome and rebuilds the list once the current signal handler is done.
fn report(state: &Rc<State>, result: Result<String, String>) {
    match result {
        Ok(msg) => toast(state, &msg),
        Err(e) => alert(state, "Something went wrong", &e),
    }
    let s = state.clone();
    glib::idle_add_local_once(move || refresh(&s));
}

fn show_list(state: &Rc<State>, all: bool) {
    state.show_all.set(all);
    state.form.replace(None);
    state.stack.set_visible_child_name("list");
    if let Some(form) = state.stack.child_by_name("form") {
        state.stack.remove(&form);
    }
    state.page.set_title(if all { "All timers" } else { "My timers" });
    state.reload.set_visible(true);
    state.save.set_visible(false);
    state.cancel.set_visible(false);
    state.nav.select_row(state.nav.row_at_index(if all { ALL } else { MINE }).as_ref());
    refresh(state);
}

/// Shows the form for a new timer, or for editing one made by this app.
fn open_form(state: &Rc<State>, spec: Spec, existing: bool, preset: u32) {
    let s = state.clone();
    let form = form::build(spec, existing, preset, move |spec, preset| {
        // The form being replaced is still handling the signal, so swap it afterwards.
        let s = s.clone();
        glib::idle_add_local_once(move || open_form(&s, spec, false, preset));
    });
    if let Some(old) = state.stack.child_by_name("form") {
        state.stack.remove(&old);
    }
    state.stack.add_named(&form.page, Some("form"));
    state.stack.set_visible_child_name("form");
    state.form.replace(Some((form.spec, existing)));
    state.page.set_title(if existing { "Edit timer" } else { "New timer" });
    state.reload.set_visible(false);
    state.save.set_visible(true);
    state.cancel.set_visible(true);
    if !existing {
        state.nav.select_row(state.nav.row_at_index(NEW).as_ref());
    }
    state.split.set_show_content(true);
}

fn refresh(state: &Rc<State>) {
    state.list.remove_all();
    let needle = state.search.borrow().trim().to_lowercase();
    let mut shown = 0;
    for t in systemd::list_timers(state.show_all.get()) {
        if !needle.is_empty()
            && !t.name.to_lowercase().contains(&needle)
            && !t.command.to_lowercase().contains(&needle)
        {
            continue;
        }
        state.list.append(&timer_row(state, &t));
        shown += 1;
    }
    if shown == 0 {
        state.list.append(&adw::ActionRow::builder().title("No timers found").build());
    }
}

fn timer_row(state: &Rc<State>, t: &TimerInfo) -> adw::ExpanderRow {
    let row = adw::ExpanderRow::builder()
        .title(glib::markup_escape_text(&t.name).as_str())
        .subtitle(glib::markup_escape_text(&t.summary).as_str())
        .expanded(state.expanded.borrow().contains(&t.name))
        .build();
    let name = t.name.clone();
    let s = state.clone();
    row.connect_expanded_notify(move |row| {
        let mut open = s.expanded.borrow_mut();
        if row.is_expanded() {
            open.insert(name.clone());
        } else {
            open.remove(&name);
        }
    });

    if !t.deletable {
        row.add_suffix(
            &gtk::Label::builder()
                .label("System")
                .css_classes(["caption", "dim-label"])
                .build(),
        );
    }
    let switch = gtk::Switch::builder()
        .active(t.enabled())
        .valign(gtk::Align::Center)
        .tooltip_text(if t.enabled() { "Enabled" } else { "Disabled" })
        .build();
    let (s, name) = (state.clone(), t.name.clone());
    switch.connect_active_notify(move |sw| report(&s, systemd::set_enabled(&name, sw.is_active())));
    row.add_suffix(&switch);

    let command = if t.command.is_empty() { "(unknown)" } else { &t.command };
    for (title, value) in [
        ("Command", command.to_string()),
        ("Next run", format_time(t.next)),
        ("Last run", format_time(t.last)),
    ] {
        row.add_row(
            &adw::ActionRow::builder()
                .title(title)
                .subtitle(glib::markup_escape_text(&value).as_str())
                .subtitle_selectable(true)
                .css_classes(["property"])
                .build(),
        );
    }

    let buttons = gtk::Box::builder()
        .spacing(8)
        .margin_top(10)
        .margin_bottom(10)
        .margin_start(12)
        .margin_end(12)
        .build();
    let button = |label: &str, style: Option<&str>| {
        let b = gtk::Button::builder().label(label).css_classes(["pill"]).build();
        if let Some(style) = style {
            b.add_css_class(style);
        }
        buttons.append(&b);
        b
    };

    let run = button("Run now", Some("suggested-action"));
    let (s, name) = (state.clone(), t.name.clone());
    run.connect_clicked(move |_| {
        if s.running.get() {
            toast(&s, "A job is already running");
            return;
        }
        s.running.set(true);
        toast(&s, &format!("Running {name}…"));
        let (s, name) = (s.clone(), name.clone());
        glib::spawn_future_local(async move {
            let job = name.clone();
            let out = gio::spawn_blocking(move || systemd::run_now(&job)).await.unwrap_or_default();
            s.running.set(false);
            show_text(&s, &format!("Output: {name}"), &out);
            refresh(&s);
        });
    });

    let logs = button("Logs", None);
    let (s, name) = (state.clone(), t.name.clone());
    logs.connect_clicked(move |_| show_text(&s, &format!("Logs: {name}"), &systemd::logs(&name)));

    // Only timers with a unit file in the user directory can be edited or deleted.
    if t.deletable {
        let edit = button("Edit", None);
        let (s, name) = (state.clone(), t.name.clone());
        edit.connect_clicked(move |_| match Spec::load(&name) {
            Some(spec) => open_form(&s, spec, true, 0),
            None => report(&s, systemd::open_in_editor(&name)),
        });

        let delete = button("Delete", Some("destructive-action"));
        let (s, name) = (state.clone(), t.name.clone());
        delete.connect_clicked(move |_| confirm_delete(&s, &name));
    }

    row.add_row(&gtk::ListBoxRow::builder().activatable(false).child(&buttons).build());
    row
}

fn show_text(state: &State, title: &str, text: &str) {
    let view = gtk::TextView::builder()
        .editable(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .top_margin(12)
        .bottom_margin(12)
        .left_margin(12)
        .right_margin(12)
        .build();
    view.buffer().set_text(text);
    let body = adw::ToolbarView::new();
    body.add_top_bar(&adw::HeaderBar::new());
    body.set_content(Some(&gtk::ScrolledWindow::builder().child(&view).vexpand(true).build()));
    let dialog = adw::Dialog::builder()
        .title(title)
        .content_width(760)
        .content_height(500)
        .child(&body)
        .build();
    dialog.present(Some(&state.window));
}

fn confirm_delete(state: &Rc<State>, name: &str) {
    let mut body = format!("{name} and its service will be removed. This cannot be undone.");
    if !systemd::is_managed(name) {
        body.push_str("\n\nIt wasn't made by this app: its unit files in ~/.config/systemd/user will be deleted.");
    }
    let dialog = adw::AlertDialog::new(Some("Delete timer?"), Some(&body));
    dialog.add_responses(&[("cancel", "Cancel"), ("delete", "Delete")]);
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    let (s, name) = (state.clone(), name.to_string());
    dialog.connect_response(None, move |_, response| {
        if response == "delete" {
            report(&s, systemd::delete(&name));
        }
    });
    dialog.present(Some(&state.window));
}
