# Builds offline: the crates listed in Cargo.lock come vendored in Source1.
# Both tarballs are made by .copr/Makefile (make -f .copr/Makefile srpm outdir=DIR):
#   Source0: git archive of the repo
#   Source1: the output of "cargo vendor" in that tree

# Rust binaries carry no useful debuginfo here.
%global debug_package %{nil}

Name:           timer-manager
Version:        0.4.1
Release:        1%{?dist}
Summary:        Create and manage systemd user timers

# GPL-3.0-only is the app itself. The rest covers the Rust crates compiled into
# the binary; for dual-licensed crates one option is listed.
License:        GPL-3.0-only AND MIT AND Unicode-3.0
URL:            https://github.com/Xarianne/timer-manager
Source0:        %{name}-%{version}.tar.gz
Source1:        %{name}-%{version}-vendor.tar.gz

BuildRequires:  cargo
BuildRequires:  rust >= 1.95
BuildRequires:  gcc
BuildRequires:  desktop-file-utils
BuildRequires:  pkgconfig(gtk4) >= 4.12
BuildRequires:  pkgconfig(libadwaita-1) >= 1.6

# GTK and libadwaita are linked normally, so rpm adds those dependencies itself.
Requires:       systemd

%description
Timer Manager is a small desktop app for creating and managing systemd user
timers without writing unit files by hand. It lists your timers with an on/off
switch and their next and last run time, runs a job on demand and shows its
output, and has a form with presets, a script picker, service and timer
options, and raw extra settings. Settings are checked with systemd-analyze
before anything is installed.

Timers it creates are ordinary files in ~/.config/systemd/user, so they keep
working without the app.

%prep
%autosetup -a1
mkdir -p .cargo
cat > .cargo/config.toml <<'EOF'
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"
EOF

%build
cargo build --release --locked --offline

%install
install -Dpm755 target/release/timer-manager %{buildroot}%{_bindir}/timer-manager
install -Dpm644 packaging/io.github.xarianne.TimerManager.desktop %{buildroot}%{_datadir}/applications/io.github.xarianne.TimerManager.desktop

%check
desktop-file-validate %{buildroot}%{_datadir}/applications/io.github.xarianne.TimerManager.desktop
# The unit tests create and delete real timers in the user's systemd instance,
# which a build chroot does not have, so they are not run here.

%files
%license LICENSE
%doc README.md
%{_bindir}/timer-manager
%{_datadir}/applications/io.github.xarianne.TimerManager.desktop

%changelog
* Sun Oct 25 2026 xariann <xariann.widely103@passmail.com> - 0.4.1-1
- Show the System label before the switch so switches line up across rows

* Sun Oct 25 2026 xariann <xariann.widely103@passmail.com> - 0.4.0-1
- Rebuild the interface with GTK 4 and libadwaita, following the system's dark mode, accent color and icons
- Pick scripts with the desktop's own file chooser; kdialog or zenity are no longer needed
- Rename the desktop file to io.github.xarianne.TimerManager.desktop to match the app ID

* Sat Oct 24 2026 xariann <xariann.widely103@passmail.com> - 0.3.2-1
- Open terminal text editors like nano or Fresh in a terminal when editing a timer outside Plasma and GNOME

* Tue Oct 20 2026 xariann <xariann.widely103@passmail.com> - 0.3.1-1
- Find the DMS fonts in the renamed DCommon directory as well as the older DankCommon one

* Tue Oct 20 2026 xariann <xariann.widely103@passmail.com> - 0.3.0-1
- Enable copy and paste in the text fields
- Give the Command field its own full-width line so long commands fit

* Tue Oct 20 2026 xariann <xariann.widely103@passmail.com> - 0.2.0-1
- Show normal window decorations outside tiling compositors such as Plasma
- Use a fixed 92% opaque window background instead of the DMS setting

* Tue Oct 20 2026 xariann <xariann.widely103@passmail.com> - 0.1.1-1
- Hide the Edit button for system timers and label them with a System badge

* Tue Oct 20 2026 xariann <xariann.widely103@passmail.com> - 0.1.0-1
- Initial package
