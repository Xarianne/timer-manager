use std::{fs, path::PathBuf, process::Command};

use serde_json::Value;

pub const MARKER: &str = "# Managed by timer-manager";
pub const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const WEEKDAYS_LONG: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

pub fn unit_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(".config/systemd/user")
}

fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("{program}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let out = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Err(if err.is_empty() { out } else { err })
    }
}

fn sctl(args: &[&str]) -> Result<String, String> {
    let mut full = vec!["--user", "--no-pager"];
    full.extend_from_slice(args);
    run("systemctl", &full)
}

#[derive(Clone)]
pub struct TimerInfo {
    pub name: String,
    pub state: String,
    pub next: Option<u64>,
    pub last: Option<u64>,
    /// Plain-English schedule for timers made here, else the raw timer lines.
    pub summary: String,
    pub command: String,
    pub managed: bool,
    /// The timer has a unit file in ~/.config/systemd/user, so it can be removed.
    pub deletable: bool,
}

impl TimerInfo {
    pub fn enabled(&self) -> bool {
        self.state == "enabled"
    }
}

fn parse_json(text: Result<String, String>) -> Value {
    text.ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null)
}

fn nonzero(v: &Value) -> Option<u64> {
    v.as_u64().filter(|n| *n > 0)
}

fn schedule_of(unit: &str) -> String {
    let text = sctl(&["cat", unit]).unwrap_or_default();
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("On") && l.contains('='))
        .collect();
    lines.join("; ")
}

/// What the paired service runs: the saved command for app-made timers, else its ExecStart lines.
fn command_of(name: &str, managed: bool) -> String {
    if managed {
        if let Some(spec) = Spec::load(name) {
            return spec.command;
        }
    }
    let text = sctl(&["cat", &format!("{name}.service")]).unwrap_or_default();
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("ExecStart="))
        .map(|l| l.trim_start_matches("ExecStart=").trim_start_matches(['-', '@', ':', '+', '!']))
        .collect();
    lines.join("; ")
}

pub fn is_managed(name: &str) -> bool {
    fs::read_to_string(unit_dir().join(format!("{name}.timer")))
        .map(|t| t.contains(MARKER))
        .unwrap_or(false)
}

pub fn list_timers(show_all: bool) -> Vec<TimerInfo> {
    let files = parse_json(sctl(&["list-unit-files", "--type=timer", "--output=json"]));
    let running = parse_json(sctl(&["list-timers", "--all", "--output=json"]));
    let dir = unit_dir();

    let mut timers = Vec::new();
    for f in files.as_array().into_iter().flatten() {
        let Some(unit) = f["unit_file"].as_str() else { continue };
        let Some(name) = unit.strip_suffix(".timer") else { continue };
        if !show_all && !dir.join(unit).exists() {
            continue;
        }
        let live = running
            .as_array()
            .and_then(|a| a.iter().find(|e| e["unit"].as_str() == Some(unit)));
        let managed = is_managed(name);
        let schedule = schedule_of(unit);
        let summary = managed
            .then(|| Spec::load(name).map(|s| s.describe()))
            .flatten()
            .unwrap_or(schedule);
        timers.push(TimerInfo {
            summary,
            command: command_of(name, managed),
            managed,
            deletable: dir.join(unit).exists(),
            name: name.to_string(),
            state: f["state"].as_str().unwrap_or("?").to_string(),
            next: live.and_then(|e| nonzero(&e["next"])),
            last: live.and_then(|e| nonzero(&e["last"])),
        });
    }
    timers.sort_by(|a, b| b.managed.cmp(&a.managed).then(a.name.cmp(&b.name)));
    timers
}

/// Runs the service to completion and returns a report: outcome plus the log lines from this run.
pub fn run_now(name: &str) -> String {
    let unit = format!("{name}.service");
    let since = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let started = sctl(&["start", &unit]);

    let prop = |p: &str| {
        sctl(&["show", &unit, "-p", p, "--value"])
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    let (result, code) = (prop("Result"), prop("ExecMainStatus"));
    let headline = match (&started, result.as_str()) {
        (Ok(_), "success") => format!("Succeeded (exit code {code})"),
        (Ok(_), r) => format!("Finished: {r} (exit code {code})"),
        (Err(e), r) => format!("Failed: {r} (exit code {code})\n{e}"),
    };
    let log = run(
        "journalctl",
        &["--user", "-u", &unit, "--since", &since, "--no-pager", "-o", "short"],
    )
    .unwrap_or_default();
    let log = if log.trim().is_empty() { "(no output)".into() } else { log };
    format!("{headline}\n\n{log}")
}

pub fn set_enabled(name: &str, enable: bool) -> Result<String, String> {
    let unit = format!("{name}.timer");
    if enable {
        sctl(&["enable", "--now", &unit])?;
        Ok(format!("Enabled {name}"))
    } else {
        sctl(&["disable", "--now", &unit])?;
        Ok(format!("Disabled {name}"))
    }
}

/// Picks up hand-edited unit files.
pub fn reload() {
    let _ = sctl(&["daemon-reload"]);
}

/// Opens a timer made outside this app in the default text editor.
pub fn open_in_editor(name: &str) -> Result<String, String> {
    let path = unit_dir().join(format!("{name}.timer"));
    if !path.exists() {
        return Err("System-provided timer: no editable file in ~/.config/systemd/user".into());
    }
    // The command lives in the paired service, so open that too.
    let service = unit_dir().join(format!("{name}.service"));
    let mut files = vec![service.clone(), path];
    files.retain(|p| p.exists());
    for file in &files {
        Command::new("xdg-open")
            .arg(file)
            .spawn()
            .map_err(|e| format!("xdg-open: {e}"))?;
    }
    Ok(format!("Opened {name}.timer and .service in your editor - press Refresh after saving"))
}

pub fn delete(name: &str) -> Result<String, String> {
    if !unit_dir().join(format!("{name}.timer")).exists() {
        return Err("System-provided timer: nothing in ~/.config/systemd/user to delete".into());
    }
    let _ = sctl(&["disable", "--now", &format!("{name}.timer")]);
    let dir = unit_dir();
    let _ = fs::remove_file(dir.join(format!("{name}.timer")));
    let _ = fs::remove_file(dir.join(format!("{name}.service")));
    sctl(&["daemon-reload"])?;
    let _ = sctl(&["reset-failed", &format!("{name}.service")]);
    Ok(format!("Deleted {name}"))
}

pub fn logs(name: &str) -> String {
    let unit = format!("{name}.service");
    match run(
        "journalctl",
        &["--user", "-u", &unit, "-n", "80", "--no-pager", "-o", "short"],
    ) {
        Ok(t) if !t.trim().is_empty() => t,
        Ok(_) => "No log entries yet.".into(),
        Err(e) => e,
    }
}

/// Checks a calendar expression and returns "next run" text or the error.
pub fn preview_calendar(expr: &str) -> Result<String, String> {
    let out = run("systemd-analyze", &["calendar", expr])?;
    let next = out
        .lines()
        .find(|l| l.trim_start().starts_with("Next elapse"))
        .map(|l| l.trim().to_string())
        .unwrap_or_default();
    Ok(next)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Minutes,
    Hours,
    Daily,
    Weekly,
    Monthly,
    Custom,
}

impl Kind {
    pub const ALL: [Kind; 6] = [
        Kind::Minutes,
        Kind::Hours,
        Kind::Daily,
        Kind::Weekly,
        Kind::Monthly,
        Kind::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Minutes => "Every N minutes",
            Kind::Hours => "Every N hours",
            Kind::Daily => "Daily",
            Kind::Weekly => "Weekly",
            Kind::Monthly => "Monthly",
            Kind::Custom => "Custom (OnCalendar)",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Kind::Minutes => "minutes",
            Kind::Hours => "hours",
            Kind::Daily => "daily",
            Kind::Weekly => "weekly",
            Kind::Monthly => "monthly",
            Kind::Custom => "custom",
        }
    }

    fn from_key(key: &str) -> Kind {
        Kind::ALL
            .into_iter()
            .find(|k| k.key() == key)
            .unwrap_or(Kind::Daily)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ServiceType {
    Oneshot,
    Simple,
    Forking,
}

impl ServiceType {
    pub const ALL: [ServiceType; 3] = [ServiceType::Oneshot, ServiceType::Simple, ServiceType::Forking];

    pub fn label(self) -> &'static str {
        match self {
            ServiceType::Oneshot => "Run once and finish",
            ServiceType::Simple => "Keep running",
            ServiceType::Forking => "Forks into background",
        }
    }

    fn key(self) -> &'static str {
        match self {
            ServiceType::Oneshot => "oneshot",
            ServiceType::Simple => "simple",
            ServiceType::Forking => "forking",
        }
    }

    fn from_key(key: &str) -> ServiceType {
        ServiceType::ALL
            .into_iter()
            .find(|t| t.key() == key)
            .unwrap_or(ServiceType::Oneshot)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Restart {
    No,
    OnFailure,
    Always,
}

impl Restart {
    pub const ALL: [Restart; 3] = [Restart::No, Restart::OnFailure, Restart::Always];

    pub fn label(self) -> &'static str {
        match self {
            Restart::No => "Never",
            Restart::OnFailure => "On failure",
            Restart::Always => "Always",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Restart::No => "no",
            Restart::OnFailure => "on-failure",
            Restart::Always => "always",
        }
    }

    fn from_key(key: &str) -> Restart {
        Restart::ALL
            .into_iter()
            .find(|r| r.key() == key)
            .unwrap_or(Restart::No)
    }
}

#[derive(Clone)]
pub struct Spec {
    pub name: String,
    pub description: String,
    pub command: String,
    pub kind: Kind,
    pub interval: u32,
    pub hour: u32,
    pub minute: u32,
    pub weekday: usize,
    pub day: u32,
    pub custom: String,
    pub persistent: bool,
    pub service_type: ServiceType,
    pub workdir: String,
    pub env: String,
    pub timeout: u32,
    pub restart: Restart,
    pub restart_delay: u32,
    pub low_priority: bool,
    pub random_delay: u32,
    pub on_boot: u32,
    pub extra_unit: String,
    pub extra_service: String,
    pub extra_timer: String,
}

impl Default for Spec {
    fn default() -> Self {
        Spec {
            name: String::new(),
            description: String::new(),
            command: String::new(),
            kind: Kind::Daily,
            interval: 1,
            hour: 9,
            minute: 0,
            weekday: 0,
            day: 1,
            custom: String::new(),
            persistent: true,
            service_type: ServiceType::Oneshot,
            workdir: String::new(),
            env: String::new(),
            timeout: 0,
            restart: Restart::No,
            restart_delay: 5,
            low_priority: false,
            random_delay: 0,
            on_boot: 0,
            extra_unit: String::new(),
            extra_service: String::new(),
            extra_timer: String::new(),
        }
    }
}

/// Multi-line values are kept on one metadata line.
fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n").replace('\r', "")
}

fn unesc(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

impl Spec {
    pub fn on_calendar(&self) -> String {
        let (h, m) = (self.hour, self.minute);
        match self.kind {
            Kind::Minutes => format!("*-*-* *:0/{}:00", self.interval.max(1)),
            Kind::Hours => format!("*-*-* 0/{}:{m:02}:00", self.interval.max(1)),
            Kind::Daily => format!("*-*-* {h:02}:{m:02}:00"),
            Kind::Weekly => format!("{} *-*-* {h:02}:{m:02}:00", WEEKDAYS[self.weekday % 7]),
            Kind::Monthly => format!("*-*-{:02} {h:02}:{m:02}:00", self.day.clamp(1, 31)),
            Kind::Custom => self.custom.trim().to_string(),
        }
    }

    /// The schedule and timer options in plain English.
    pub fn describe(&self) -> String {
        let (h, m) = (self.hour, self.minute);
        let n = self.interval.max(1);
        let mut text = match self.kind {
            Kind::Minutes if n == 1 => "Every minute.".to_string(),
            Kind::Minutes => format!("Every {n} minutes."),
            Kind::Hours if n == 1 => format!("Every hour, {m} minutes past."),
            Kind::Hours => format!("Every {n} hours from midnight, {m} minutes past."),
            Kind::Daily => format!("Every day at {h:02}:{m:02}."),
            Kind::Weekly => format!("Every {} at {h:02}:{m:02}.", WEEKDAYS_LONG[self.weekday % 7]),
            Kind::Monthly => format!("On day {} of every month at {h:02}:{m:02}.", self.day.clamp(1, 31)),
            Kind::Custom => "Custom schedule.".to_string(),
        };
        if self.on_boot > 0 {
            text.push_str(&format!(" Also {} min after the PC starts.", self.on_boot));
        }
        if self.random_delay > 0 {
            text.push_str(&format!(" Starts up to {} s late, at random.", self.random_delay));
        }
        if self.persistent {
            text.push_str(" If the PC was off, it runs when it next starts.");
        }
        text
    }

    fn metadata(&self) -> String {
        let single = |s: &str| s.replace(['\n', '\r'], " ");
        format!(
            "# tm-kind={}\n# tm-interval={}\n# tm-hour={}\n# tm-minute={}\n# tm-weekday={}\n# tm-day={}\n# tm-custom={}\n# tm-command={}\n\
             # tm-type={}\n# tm-workdir={}\n# tm-env={}\n# tm-timeout={}\n# tm-restart={}\n# tm-restart_delay={}\n# tm-lowprio={}\n# tm-random={}\n# tm-onboot={}\n\
             # tm-extra_unit={}\n# tm-extra_service={}\n# tm-extra_timer={}\n",
            self.kind.key(),
            self.interval,
            self.hour,
            self.minute,
            self.weekday,
            self.day,
            single(&self.custom),
            single(&self.command),
            self.service_type.key(),
            esc(&self.workdir),
            esc(&self.env),
            self.timeout,
            self.restart.key(),
            self.restart_delay,
            self.low_priority,
            self.random_delay,
            self.on_boot,
            esc(&self.extra_unit),
            esc(&self.extra_service),
            esc(&self.extra_timer),
        )
    }

    pub fn load(name: &str) -> Option<Spec> {
        let dir = unit_dir();
        let timer = fs::read_to_string(dir.join(format!("{name}.timer"))).ok()?;
        if !timer.contains(MARKER) {
            return None;
        }
        let mut spec = Spec { name: name.to_string(), ..Spec::default() };
        for line in timer.lines() {
            if let Some(v) = line.strip_prefix("Description=") {
                spec.description = v.to_string();
            }
            let Some(rest) = line.strip_prefix("# tm-") else { continue };
            let Some((k, v)) = rest.split_once('=') else { continue };
            match k {
                "kind" => spec.kind = Kind::from_key(v),
                "interval" => spec.interval = v.parse().unwrap_or(1),
                "hour" => spec.hour = v.parse().unwrap_or(9),
                "minute" => spec.minute = v.parse().unwrap_or(0),
                "weekday" => spec.weekday = v.parse().unwrap_or(0),
                "day" => spec.day = v.parse().unwrap_or(1),
                "custom" => spec.custom = v.to_string(),
                "command" => spec.command = v.to_string(),
                "type" => spec.service_type = ServiceType::from_key(v),
                "workdir" => spec.workdir = unesc(v),
                "env" => spec.env = unesc(v),
                "timeout" => spec.timeout = v.parse().unwrap_or(0),
                "restart" => spec.restart = Restart::from_key(v),
                "restart_delay" => spec.restart_delay = v.parse().unwrap_or(5),
                "lowprio" => spec.low_priority = v == "true",
                "random" => spec.random_delay = v.parse().unwrap_or(0),
                "onboot" => spec.on_boot = v.parse().unwrap_or(0),
                "extra_unit" => spec.extra_unit = unesc(v),
                "extra_service" => spec.extra_service = unesc(v),
                "extra_timer" => spec.extra_timer = unesc(v),
                _ => {}
            }
        }
        spec.persistent = timer.lines().any(|l| l.trim() == "Persistent=true");
        Some(spec)
    }
}

pub const PRESETS: [&str; 5] = [
    "Daily - a script every morning",
    "Nightly, low priority - good for backups",
    "Weekly - updates and cleanups (Sunday)",
    "Every hour",
    "Every 15 minutes - frequent checks",
];

/// Fills in a schedule and sensible options, keeping the name, description and command.
pub fn apply_preset(spec: &mut Spec, index: usize) {
    let kept = Spec {
        name: std::mem::take(&mut spec.name),
        description: std::mem::take(&mut spec.description),
        command: std::mem::take(&mut spec.command),
        ..Spec::default()
    };
    *spec = kept;
    match index {
        0 => {
            spec.kind = Kind::Daily;
            spec.hour = 9;
        }
        1 => {
            spec.kind = Kind::Daily;
            spec.hour = 2;
            spec.low_priority = true;
            spec.random_delay = 300;
        }
        2 => {
            spec.kind = Kind::Weekly;
            spec.weekday = 6;
            spec.hour = 10;
            spec.low_priority = true;
            spec.random_delay = 600;
        }
        3 => {
            spec.kind = Kind::Hours;
            spec.interval = 1;
            spec.minute = 0;
            spec.persistent = false;
        }
        _ => {
            spec.kind = Kind::Minutes;
            spec.interval = 15;
            spec.persistent = false;
        }
    }
}

fn shell_quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "/._-~+@:,".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// The command to run a chosen script: directly if it is executable, else via its interpreter.
pub fn script_command(path: &str) -> String {
    use std::os::unix::fs::PermissionsExt;
    let quoted = shell_quote(path);
    let executable = fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false);
    let interpreter = match std::path::Path::new(path).extension().and_then(|e| e.to_str()) {
        Some("sh") => Some("bash"),
        Some("py") => Some("python3"),
        Some("fish") => Some("fish"),
        _ => None,
    };
    match interpreter {
        Some(i) if !executable => format!("{i} {quoted}"),
        _ => quoted,
    }
}

fn exec_quote(command: &str) -> String {
    let escaped = command
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%")
        .replace('$', "$$");
    format!("ExecStart=/bin/sh -c \"{escaped}\"")
}

fn quote_env(line: &str) -> String {
    line.replace('\\', "\\\\").replace('"', "\\\"").replace('%', "%%")
}

/// The lines this app generates for each unit section, before the user's extra lines.
struct Sections {
    unit: Vec<String>,
    service: Vec<String>,
    timer: Vec<String>,
}

fn build(spec: &Spec) -> Result<Sections, String> {
    let name = spec.name.trim();
    let description = spec.description.replace(['\n', '\r'], " ");
    let description = if description.trim().is_empty() { name.to_string() } else { description };

    let unit = vec![format!("Description={description}")];

    let mut service = vec![
        format!("Type={}", spec.service_type.key()),
        "Environment=\"PATH=%h/.local/bin:%h/.cargo/bin:/usr/local/bin:/usr/bin:/bin\"".to_string(),
    ];
    let workdir = spec.workdir.trim();
    if !workdir.is_empty() {
        service.push(format!("WorkingDirectory={workdir}"));
    }
    for line in spec.env.lines().map(str::trim).filter(|l| !l.is_empty()) {
        match line.split_once('=') {
            Some((key, _)) if !key.trim().is_empty() && !key.contains(char::is_whitespace) => {
                service.push(format!("Environment=\"{}\"", quote_env(line)));
            }
            _ => return Err(format!("Environment lines need the form KEY=value: {line}")),
        }
    }
    if spec.timeout > 0 {
        service.push(format!("TimeoutStartSec={}", spec.timeout));
    }
    if spec.restart != Restart::No {
        if spec.service_type == ServiceType::Oneshot && spec.restart == Restart::Always {
            return Err("'Always' restart can't be combined with 'Run once and finish'; use 'On failure'".into());
        }
        service.push(format!("Restart={}", spec.restart.key()));
        service.push(format!("RestartSec={}", spec.restart_delay));
    }
    if spec.low_priority {
        service.push("Nice=19".into());
        service.push("IOSchedulingClass=idle".into());
    }
    service.push(exec_quote(spec.command.trim()));

    let mut timer = vec![format!("OnCalendar={}", spec.on_calendar())];
    if spec.on_boot > 0 {
        timer.push(format!("OnBootSec={}min", spec.on_boot));
    }
    if spec.random_delay > 0 {
        timer.push(format!("RandomizedDelaySec={}", spec.random_delay));
    }
    if spec.persistent {
        timer.push("Persistent=true".into());
    }

    Ok(Sections { unit, service, timer })
}

fn join(generated: &[String], extra: &str) -> String {
    let mut lines: Vec<&str> = generated.iter().map(String::as_str).collect();
    lines.extend(extra.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()));
    lines.join("\n")
}

/// Returns (service file text, timer file text).
fn render(spec: &Spec, s: &Sections) -> (String, String) {
    let service = format!(
        "{MARKER}\n[Unit]\n{}\n\n[Service]\n{}\n",
        join(&s.unit, &spec.extra_unit),
        join(&s.service, &spec.extra_service),
    );
    let timer = format!(
        "{MARKER}\n{}\n[Unit]\n{}\n\n[Timer]\n{}\n\n[Install]\nWantedBy=timers.target\n",
        spec.metadata(),
        s.unit.join("\n"),
        join(&s.timer, &spec.extra_timer),
    );
    (service, timer)
}

/// Settings that may legitimately appear more than once.
const MULTI: &[&str] = &[
    "Environment",
    "EnvironmentFile",
    "After",
    "Before",
    "Wants",
    "Requires",
    "Requisite",
    "BindsTo",
    "PartOf",
    "Conflicts",
    "ExecStartPre",
    "ExecStartPost",
    "ExecStopPost",
    "ExecCondition",
];

/// Notes on extra lines that repeat a setting an option above already writes.
pub fn warnings(spec: &Spec) -> Vec<String> {
    let Ok(s) = build(spec) else { return Vec::new() };
    let mut out = Vec::new();
    let sections = [
        ("Unit", &s.unit, &spec.extra_unit),
        ("Service", &s.service, &spec.extra_service),
        ("Timer", &s.timer, &spec.extra_timer),
    ];
    for (section, generated, extra) in sections {
        for line in extra.lines() {
            let Some((key, _)) = line.split_once('=') else { continue };
            let key = key.trim();
            if key.is_empty() || key.starts_with(['#', ';', '[']) || MULTI.contains(&key) {
                continue;
            }
            let repeated = generated
                .iter()
                .any(|g| g.split_once('=').map(|(k, _)| k == key).unwrap_or(false));
            if repeated {
                out.push(format!(
                    "[{section}] {key}= is already set by an option above; your extra line comes later and wins"
                ));
            }
        }
    }
    out
}

/// Asks systemd to check the generated files before anything is installed.
fn verify(name: &str, service: &str, timer: &str) -> Result<(), String> {
    let dir = std::env::temp_dir().join(format!("timer-manager-verify-{}-{name}", std::process::id()));
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let service_path = dir.join(format!("{name}.service"));
    let timer_path = dir.join(format!("{name}.timer"));
    let written = fs::write(&service_path, service).and_then(|_| fs::write(&timer_path, timer));
    let out = written.map_err(|e| e.to_string()).and_then(|_| {
        Command::new("systemd-analyze")
            .args(["--user", "verify"])
            .arg(&timer_path)
            .arg(&service_path)
            .output()
            .map_err(|e| format!("systemd-analyze: {e}"))
    });
    let _ = fs::remove_dir_all(&dir);
    let out = out?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    )
    .trim()
    .to_string();
    if !out.status.success() || text.contains("Unknown key") || text.contains("Unknown section") {
        Err(if text.is_empty() { "verification failed".into() } else { text })
    } else {
        Ok(())
    }
}

pub fn save(spec: &Spec, existing: bool) -> Result<String, String> {
    let name = spec.name.trim();
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("Name must use only letters, numbers, - and _".into());
    }
    if spec.command.trim().is_empty() {
        return Err("Enter a command to run".into());
    }
    let calendar = spec.on_calendar();
    if calendar.is_empty() {
        return Err("Enter a schedule".into());
    }
    preview_calendar(&calendar).map_err(|e| format!("Invalid schedule: {e}"))?;

    let dir = unit_dir();
    let timer_path = dir.join(format!("{name}.timer"));
    let service_path = dir.join(format!("{name}.service"));
    if existing {
        if !is_managed(name) {
            return Err("This timer was not created by this app".into());
        }
    } else if timer_path.exists() || service_path.exists() {
        return Err(format!("A unit named {name} already exists"));
    }

    let sections = build(spec)?;
    let (service, timer) = render(spec, &sections);
    verify(name, &service, &timer).map_err(|e| format!("systemd rejected these settings:\n{e}"))?;

    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    fs::write(&service_path, service).map_err(|e| e.to_string())?;
    fs::write(&timer_path, timer).map_err(|e| e.to_string())?;
    sctl(&["daemon-reload"])?;
    sctl(&["enable", "--now", &format!("{name}.timer")])?;
    sctl(&["restart", &format!("{name}.timer")])?;
    Ok(format!("Saved {name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_edit_delete_roundtrip() {
        let name = "tm-selftest";
        let _ = delete(name);
        let mut spec = Spec {
            name: name.into(),
            command: "echo \"hi $USER 100%\" > /dev/null".into(),
            kind: Kind::Weekly,
            weekday: 2,
            hour: 7,
            minute: 5,
            ..Spec::default()
        };
        save(&spec, false).unwrap();
        assert!(save(&spec, false).is_err(), "duplicate must be rejected");
        assert!(is_managed(name));

        let loaded = Spec::load(name).unwrap();
        assert_eq!(loaded.on_calendar(), "Wed *-*-* 07:05:00");
        assert_eq!(loaded.command, spec.command);

        let info = list_timers(false).into_iter().find(|t| t.name == name).unwrap();
        assert!(info.enabled() && info.managed && info.next.is_some());

        spec.kind = Kind::Minutes;
        spec.interval = 30;
        save(&spec, true).unwrap();
        assert_eq!(Spec::load(name).unwrap().on_calendar(), "*-*-* *:0/30:00");

        let report = run_now(name);
        assert!(report.starts_with("Succeeded (exit code 0)"), "{report}");
        let failing = Spec { command: "echo boom; exit 3".into(), ..spec.clone() };
        save(&failing, true).unwrap();
        let report = run_now(name);
        assert!(report.starts_with("Failed") && report.contains("boom"), "{report}");
        assert!(save(&Spec { custom: "nonsense".into(), kind: Kind::Custom, ..spec.clone() }, true).is_err());

        delete(name).unwrap();
        assert!(!unit_dir().join(format!("{name}.timer")).exists());
        assert!(list_timers(false).iter().all(|t| t.name != name));
    }

    #[test]
    fn presets_descriptions_and_scripts() {
        let mut spec = Spec { name: "n".into(), command: "c".into(), hour: 5, ..Spec::default() };
        apply_preset(&mut spec, 1);
        assert_eq!((spec.name.as_str(), spec.command.as_str()), ("n", "c"));
        assert_eq!(spec.on_calendar(), "*-*-* 02:00:00");
        assert!(spec.low_priority);
        assert_eq!(
            spec.describe(),
            "Every day at 02:00. Starts up to 300 s late, at random. If the PC was off, it runs when it next starts."
        );

        apply_preset(&mut spec, 2);
        assert_eq!(spec.on_calendar(), "Sun *-*-* 10:00:00");
        assert!(spec.describe().starts_with("Every Sunday at 10:00."));

        apply_preset(&mut spec, 4);
        assert_eq!(spec.describe(), "Every 15 minutes.");
        assert!(!spec.low_priority && spec.random_delay == 0, "preset must reset earlier options");
        for i in 0..PRESETS.len() {
            apply_preset(&mut spec, i);
            preview_calendar(&spec.on_calendar()).unwrap();
        }

        assert_eq!(shell_quote("/home/me/run.sh"), "/home/me/run.sh");
        assert_eq!(shell_quote("/home/me/my script.sh"), "'/home/me/my script.sh'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        // A missing .sh file isn't executable, so it goes through bash.
        assert_eq!(script_command("/nonexistent/x.sh"), "bash /nonexistent/x.sh");
        assert_eq!(script_command("/nonexistent/x.py"), "python3 /nonexistent/x.py");
        assert_eq!(script_command("/nonexistent/prog"), "/nonexistent/prog");
        assert_eq!(script_command("/bin/sh"), "/bin/sh");
    }

    #[test]
    fn options_and_extra_settings() {
        let name = "tm-selftest-opts";
        let _ = delete(name);
        let spec = Spec {
            name: name.into(),
            command: "echo \"FOO=$FOO dir=$(pwd)\"".into(),
            workdir: "/tmp".into(),
            env: "FOO=bar baz\nX=1".into(),
            timeout: 30,
            restart: Restart::OnFailure,
            restart_delay: 3,
            low_priority: true,
            random_delay: 10,
            on_boot: 5,
            extra_service: "ProtectHome=read-only\nUMask=0077".into(),
            extra_timer: "AccuracySec=1min".into(),
            extra_unit: "Documentation=man:systemd.timer(5)".into(),
            ..Spec::default()
        };
        save(&spec, false).unwrap();

        let service = fs::read_to_string(unit_dir().join(format!("{name}.service"))).unwrap();
        for line in [
            "Type=oneshot",
            "WorkingDirectory=/tmp",
            "Environment=\"FOO=bar baz\"",
            "TimeoutStartSec=30",
            "Restart=on-failure",
            "RestartSec=3",
            "Nice=19",
            "IOSchedulingClass=idle",
            "ProtectHome=read-only",
            "Documentation=man:systemd.timer(5)",
        ] {
            assert!(service.lines().any(|l| l == line), "missing {line}\n{service}");
        }
        let timer = fs::read_to_string(unit_dir().join(format!("{name}.timer"))).unwrap();
        for line in ["OnBootSec=5min", "RandomizedDelaySec=10", "Persistent=true", "AccuracySec=1min"] {
            assert!(timer.lines().any(|l| l == line), "missing {line}\n{timer}");
        }

        let loaded = Spec::load(name).unwrap();
        assert_eq!(loaded.env, spec.env);
        assert_eq!(loaded.extra_service, spec.extra_service);
        assert_eq!(loaded.extra_timer, spec.extra_timer);
        assert_eq!(loaded.extra_unit, spec.extra_unit);
        assert_eq!(loaded.workdir, "/tmp");
        assert!(loaded.low_priority && loaded.restart == Restart::OnFailure);
        assert_eq!((loaded.timeout, loaded.restart_delay, loaded.random_delay, loaded.on_boot), (30, 3, 10, 5));

        let report = run_now(name);
        assert!(report.starts_with("Succeeded"), "{report}");
        assert!(report.contains("FOO=bar baz dir=/tmp"), "{report}");

        let bad = |change: fn(&mut Spec)| {
            let mut s = Spec { name: name.into(), ..spec.clone() };
            change(&mut s);
            save(&s, true)
        };
        assert!(bad(|s| s.extra_service = "NotARealKey=1".into()).is_err(), "unknown key accepted");
        assert!(bad(|s| s.env = "NOEQUALS".into()).is_err());
        assert!(bad(|s| s.restart = Restart::Always).is_err());
        assert!(Spec::load(name).unwrap().extra_service == spec.extra_service, "failed save changed files");

        let dup = Spec { extra_service: "Type=simple".into(), ..spec.clone() };
        assert_eq!(warnings(&dup).len(), 1);
        assert!(warnings(&spec).is_empty());

        delete(name).unwrap();
        assert!(list_timers(false).iter().all(|t| t.name != name));
    }
}
