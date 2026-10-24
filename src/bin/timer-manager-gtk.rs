//! GTK4/libadwaita prototype of the timer list, to compare with the egui app.
//! Run it with `cargo run --bin timer-manager-gtk`.

#[allow(dead_code)]
#[path = "../systemd.rs"]
mod systemd;

use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    rc::Rc,
};

use adw::prelude::*;
use chrono::{Local, TimeZone};
use gtk::{gio, glib};

const APP_ID: &str = "io.github.xarianne.TimerManager.Gtk";

struct State {
    window: adw::ApplicationWindow,
    page: adw::NavigationPage,
    list: gtk::ListBox,
    toasts: adw::ToastOverlay,
    show_all: Cell<bool>,
    search: RefCell<String>,
    /// Rows the user opened, so a refresh keeps them open.
    expanded: RefCell<HashSet<String>>,
}

fn main() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_window);
    app.run()
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

fn build_window(app: &adw::Application) {
    if let Some(window) = app.active_window() {
        window.present();
        return;
    }

    // Sidebar: search and the two lists.
    let search = gtk::SearchEntry::builder().placeholder_text("Search timers").build();
    let nav = gtk::ListBox::builder().css_classes(["navigation-sidebar"]).build();
    for label in ["My timers", "All timers"] {
        nav.append(&gtk::Label::builder().label(label).xalign(0.0).build());
    }
    let sidebar_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();
    search.set_margin_start(12);
    search.set_margin_end(12);
    sidebar_box.append(&search);
    sidebar_box.append(&nav);
    let sidebar_view = adw::ToolbarView::new();
    sidebar_view.add_top_bar(&adw::HeaderBar::new());
    sidebar_view.set_content(Some(&sidebar_box));
    let sidebar = adw::NavigationPage::builder()
        .title("Timer Manager")
        .child(&sidebar_view)
        .build();

    // Content: the timers as one boxed list.
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
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&scroll));
    let header = adw::HeaderBar::new();
    let reload = gtk::Button::builder()
        .icon_name("view-refresh-symbolic")
        .tooltip_text("Refresh")
        .build();
    header.pack_end(&reload);
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
        .default_height(680)
        .width_request(360)
        .height_request(300)
        .content(&split)
        .build();
    let narrow = adw::Breakpoint::new(adw::BreakpointCondition::parse("max-width: 600sp").unwrap());
    narrow.add_setter(&split, "collapsed", Some(&true.to_value()));
    window.add_breakpoint(narrow);

    let state = Rc::new(State {
        window: window.clone(),
        page,
        list,
        toasts,
        show_all: Cell::new(false),
        search: RefCell::new(String::new()),
        expanded: RefCell::new(HashSet::new()),
    });

    let s = state.clone();
    nav.connect_row_selected(move |_, row| {
        let Some(row) = row else { return };
        let all = row.index() == 1;
        s.show_all.set(all);
        s.page.set_title(if all { "All timers" } else { "My timers" });
        refresh(&s);
    });
    let s = state.clone();
    search.connect_search_changed(move |entry| {
        *s.search.borrow_mut() = entry.text().to_string();
        refresh(&s);
    });
    let s = state.clone();
    reload.connect_clicked(move |_| {
        systemd::reload();
        refresh(&s);
    });

    nav.select_row(nav.row_at_index(0).as_ref());
    window.present();
}

fn toast(state: &State, text: &str) {
    state.toasts.add_toast(adw::Toast::new(text));
}

/// Shows the outcome and rebuilds the list once the current signal handler is done.
fn report(state: &Rc<State>, result: Result<String, String>) {
    match result {
        Ok(msg) => toast(state, &msg),
        Err(e) => toast(state, &format!("Error: {e}")),
    }
    let s = state.clone();
    glib::idle_add_local_once(move || refresh(&s));
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

fn timer_row(state: &Rc<State>, t: &systemd::TimerInfo) -> adw::ExpanderRow {
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
    run.connect_clicked(move |b| {
        b.set_sensitive(false);
        toast(&s, &format!("Running {name}…"));
        let (s, name, b) = (s.clone(), name.clone(), b.clone());
        glib::spawn_future_local(async move {
            let job = name.clone();
            let out = gio::spawn_blocking(move || systemd::run_now(&job)).await.unwrap_or_default();
            b.set_sensitive(true);
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
        edit.connect_clicked(move |_| match systemd::Spec::load(&name) {
            Some(_) => toast(&s, "The edit form isn't in this prototype yet"),
            None => report(&s, systemd::open_in_editor(&name)),
        });

        let delete = button("Delete", Some("destructive-action"));
        let (s, name) = (state.clone(), t.name.clone());
        delete.connect_clicked(move |_| confirm_delete(&s, &name));
    }

    let actions = gtk::ListBoxRow::builder().activatable(false).child(&buttons).build();
    row.add_row(&actions);
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
