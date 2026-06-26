Name:           systemd-compat
Version:        0.1.0
Release:        1%{?dist}
Summary:        Dummy provider for libsystemd on Forge-based systems
License:        MIT
URL:            https://github.com/sisyphuscode/Sisyphus-Linux
BuildArch:      noarch

%description
Metapackage placeholder so DNF dependency solvers that expect systemd
libraries can be satisfied on Sisyphus Linux images using Forge as PID 1.

%files
%ghost %{_libdir}/systemd/libsystemd.so.0

%changelog
* Fri Jun 26 2026 Kenny Glowner <sisyphuscode@fedoraproject.org> - 0.1.0-1
- Initial dummy systemd compatibility provider
