# Builds offline: the crates listed in Cargo.lock come vendored in Source1.
# Both tarballs are made by .copr/Makefile (make -f .copr/Makefile srpm outdir=DIR):
#   Source0: git archive of the repo
#   Source1: the output of "cargo vendor" in that tree

# Rust binaries carry no useful debuginfo here.
%global debug_package %{nil}

Name:           timer-manager
Version:        0.3.2
Release:        1%{?dist}
Summary:        Create and manage systemd user timers

# GPL-3.0-only is the app itself. The rest covers the Rust crates and fonts
# compiled into the binary; for dual-licensed crates one option is listed.
License:        GPL-3.0-only AND Apache-2.0 AND BSL-1.0 AND ISC AND MIT AND OFL-1.1 AND Ubuntu-font-1.0 AND Unicode-3.0 AND Zlib
URL:            https://github.com/Xarianne/timer-manager
Source0:        %{name}-%{version}.tar.gz
Source1:        %{name}-%{version}-vendor.tar.gz

BuildRequires:  cargo
BuildRequires:  rust >= 1.95
BuildRequires:  gcc
BuildRequires:  desktop-file-utils

Requires:       systemd
Requires:       fontconfig
# The app loads its graphics libraries at runtime, so rpm cannot detect them.
Requires:       libwayland-client
Requires:       libwayland-egl
Requires:       libxkbcommon
Requires:       libglvnd-egl
Requires:       libglvnd-glx
# X11 fallback
Recommends:     libX11
Recommends:     libX11-xcb
Recommends:     libxcb
Recommends:     libXcursor
Recommends:     libXi
Recommends:     libXrender
Recommends:     libxkbcommon-x11
Recommends:     (kdialog or zenity)

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
install -Dpm644 packaging/timer-manager.desktop %{buildroot}%{_datadir}/applications/timer-manager.desktop

%check
desktop-file-validate %{buildroot}%{_datadir}/applications/timer-manager.desktop
# The unit tests create and delete real timers in the user's systemd instance,
# which a build chroot does not have, so they are not run here.

%files
%license LICENSE
%doc README.md
%{_bindir}/timer-manager
%{_datadir}/applications/timer-manager.desktop

%changelog
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
