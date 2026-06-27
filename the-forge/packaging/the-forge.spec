Name:           the-forge
Version:        2.0.0
Release:        7%{?dist}
Summary:        The Forge — Rust PID 1 init system

License:        MIT
URL:            https://github.com/SisyphusCode/the-forge
Source0:        {{{ git_repo_archive source_name=the-forge-$(cat VERSION).tar.gz }}}

# Rust binary; no C debugsource to package.
%global debug_package %{nil}

BuildRequires:  rust >= 1.75
BuildRequires:  cargo
# Optional (for `make check`): rustfmt clippy
# On RHEL: dnf install rustfmt clippy

%description
High-performance PID 1 init with parallel DAG boot, socket activation,
systemd unit import, cgroups v2, journal, and forgectl IPC.

%prep
{{{ git_repo_setup_macro source_indices=0 }}}

%build
export SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:-1}
cargo build --locked --release --workspace

%install
install -d %{buildroot}%{_sbindir} %{buildroot}%{_bindir} %{buildroot}%{_libexecdir}
install -d %{buildroot}%{_sysconfdir}/pam.d
install -d %{buildroot}%{_sysconfdir}/forge/{units,systemd,backup}
install -d %{buildroot}%{_prefix}/lib/dracut/modules.d/90forge
install -d %{buildroot}%{_datadir}/forge/dbus-overrides
install -d %{buildroot}%{_datadir}/forge
install -d %{buildroot}%{_libexecdir}/forge

install -m 0755 target/release/forge-core %{buildroot}%{_sbindir}/forge-core
install -m 0755 target/release/forgectl %{buildroot}%{_bindir}/forgectl
install -m 0755 target/release/forge-logind %{buildroot}%{_bindir}/forge-logind
install -m 0755 target/release/forge-journalctl %{buildroot}%{_bindir}/forge-journalctl

install -m 0755 packaging/pam/forge-session-open %{buildroot}%{_libexecdir}/forge-session-open
install -m 0755 packaging/pam/forge-session-close %{buildroot}%{_libexecdir}/forge-session-close
install -m 0755 packaging/pam/pam-forge-login-session.sh %{buildroot}%{_libexecdir}/forge/pam-forge-login-session.sh
install -m 0755 packaging/pam/pam-logind-create-session.py %{buildroot}%{_libexecdir}/forge/pam-logind-create-session.py
install -m 0644 packaging/pam/login-forge-snippet %{buildroot}%{_datadir}/forge/login-forge-snippet
install -m 0644 packaging/pam/forge %{buildroot}%{_sysconfdir}/pam.d/forge

cp -a packaging/ciq/*.sh packaging/ciq/*.py %{buildroot}%{_libexecdir}/forge/ 2>/dev/null || :
install -m 0755 packaging/ciq/start-gdm.sh %{buildroot}%{_libexecdir}/forge/start-gdm.sh

# Native mode dbus overrides — installed via %%post (avoid RPM file conflicts with systemd).
install -m 0644 packaging/dbus/org.freedesktop.systemd1-native.service \
  %{buildroot}%{_datadir}/forge/dbus-overrides/org.freedesktop.systemd1.service
install -m 0644 packaging/dbus/session-org.freedesktop.systemd1.service \
  %{buildroot}%{_datadir}/forge/dbus-overrides/session-org.freedesktop.systemd1.service

install -m 0644 forge-core/examples/default.target %{buildroot}%{_sysconfdir}/forge/default.target
install -m 0644 forge-core/examples/network.toml %{buildroot}%{_sysconfdir}/forge/network.toml
install -m 0644 forge-core/examples/desktop.toml %{buildroot}%{_sysconfdir}/forge/desktop.toml
cp -a forge-core/examples/units/. %{buildroot}%{_sysconfdir}/forge/units/
cp -a forge-core/examples/systemd/. %{buildroot}%{_sysconfdir}/forge/systemd/
install -m 0755 packaging/dracut/90forge/module-setup.sh %{buildroot}%{_prefix}/lib/dracut/modules.d/90forge/
install -m 0755 packaging/dracut/90forge/forge-cmdline.sh %{buildroot}%{_prefix}/lib/dracut/modules.d/90forge/

%files
%doc README.md NATIVE_MODE.md
%{_sbindir}/forge-core
%{_bindir}/forgectl
%{_bindir}/forge-logind
%{_bindir}/forge-journalctl
%{_libexecdir}/forge-session-open
%{_libexecdir}/forge-session-close
%dir %{_libexecdir}/forge
%{_libexecdir}/forge/*
%config(noreplace) %{_sysconfdir}/pam.d/forge
%dir %{_sysconfdir}/forge
%dir %{_sysconfdir}/forge/units
%dir %{_sysconfdir}/forge/systemd
%{_sysconfdir}/forge/default.target
%{_sysconfdir}/forge/network.toml
%{_sysconfdir}/forge/desktop.toml
%{_sysconfdir}/forge/units/*
%{_sysconfdir}/forge/systemd/*
%{_datadir}/forge/login-forge-snippet
%{_datadir}/forge/dbus-overrides/*
%{_prefix}/lib/dracut/modules.d/90forge/*

%post
install -d %{_sysconfdir}/dbus-1/system-services 2>/dev/null || true
install -m 0644 %{_datadir}/forge/dbus-overrides/org.freedesktop.systemd1.service \
  %{_sysconfdir}/dbus-1/system-services/org.freedesktop.systemd1.service 2>/dev/null || true
install -m 0644 %{_datadir}/forge/dbus-overrides/org.freedesktop.systemd1.service \
  %{_datadir}/dbus-1/system-services/org.freedesktop.systemd1.service 2>/dev/null || true
install -m 0644 %{_datadir}/forge/dbus-overrides/session-org.freedesktop.systemd1.service \
  %{_datadir}/dbus-1/services/org.freedesktop.systemd1.service 2>/dev/null || true

%changelog
* Fri Jun 26 2026 Kenny Glowner <sisyphuscode@fedoraproject.org> - 2.0.0-7
- Use Python systemd1-stub for logind session scopes when installed
- systemd1-stub forge unit owns org.freedesktop.systemd1 before logind

* Fri Jun 26 2026 Kenny Glowner <sisyphuscode@fedoraproject.org> - 2.0.0-6
- Fix StartTransientUnit for logind session scopes (do not start as forge service)

* Fri Jun 26 2026 Kenny Glowner <sisyphuscode@fedoraproject.org> - 2.0.0-5
- Ship dbus overrides, systemd1 stubs, and PAM logind helpers in RPM
- Native org.freedesktop.systemd1 dbus service (forge-core owns the name)
- Fix StartTransientUnit session scope leader PID cgroup attach

* Fri Jun 26 2026 Kenny Glowner <sisyphuscode@fedoraproject.org> - 2.0.0-4
- Escape %%install in changelog (fixes EPEL/RHEL spec parse)
- Use rpkg git archive macros so COPR SCM builds generate Source0 from git
- Fix COPR/RPM install paths and suppress debugsource for Rust builds

* Wed Jun 24 2026 SisyphusCode <SisyphusCode0311@gmail.com> - 2.0.0-1
- D-Bus compat bridge, timers, mount units, drop-ins, udev rules, journalctl, PAM hooks

* Wed Jun 24 2026 SisyphusCode <SisyphusCode0311@gmail.com> - 1.0.0-1
- First production edition