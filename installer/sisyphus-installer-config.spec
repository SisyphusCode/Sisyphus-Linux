Name:           sisyphus-installer-config
Version:        1.0.0
Release:        3%{?dist}
Summary:        Calamares configuration and branding for Sisyphus Linux
License:        GPLv3
URL:            https://github.com/SisyphusCode/Sisyphus-Linux
Source0:        %{name}-%{version}.tar.gz
BuildArch:      noarch

Requires:       calamares

%description
Custom settings, branding, and module configurations for the Sisyphus Linux deployment.
Includes systemd-free module execution paths and the official Sisyphus logo.

%prep
# SRPM: extract Source0 tarball.
# SCM mono-repo: copy installer/ from repo root or from spec directory.
if [ -f %{SOURCE0} ]; then
    %autosetup -c
elif [ -d %{_sourcedir}/installer ]; then
    mkdir -p %{_builddir}/%{name}-%{version}
    cp -a %{_sourcedir}/installer %{_builddir}/%{name}-%{version}/
elif [ -f %{_sourcedir}/settings.conf ]; then
    mkdir -p %{_builddir}/%{name}-%{version}/installer
    cp -a %{_sourcedir}/branding %{_sourcedir}/settings.conf %{_builddir}/%{name}-%{version}/installer/
else
    echo "ERROR: cannot locate installer sources under %{_sourcedir}" >&2
    exit 1
fi

%build
# Nothing to compile

%install
rm -rf %{buildroot}

mkdir -p %{buildroot}/etc/calamares/
mkdir -p %{buildroot}/usr/share/calamares/branding/sisyphus/

install -p -m 644 installer/settings.conf %{buildroot}/etc/calamares/
cp -a installer/modules %{buildroot}/etc/calamares/
install -p -m 644 installer/branding/sisyphus/branding.desc %{buildroot}/usr/share/calamares/branding/sisyphus/
install -p -m 644 installer/branding/sisyphus/logo.png %{buildroot}/usr/share/calamares/branding/sisyphus/
install -p -m 644 installer/branding/sisyphus/show.qml %{buildroot}/usr/share/calamares/branding/sisyphus/

%files
%defattr(-,root,root,-)
%dir /usr/share/calamares/branding/sisyphus
/etc/calamares/settings.conf
/etc/calamares/modules
/usr/share/calamares/branding/sisyphus/branding.desc
/usr/share/calamares/branding/sisyphus/logo.png
/usr/share/calamares/branding/sisyphus/show.qml

%changelog
* Sat Jun 27 2026 Kenny Glowner <sisyphuscode@fedoraproject.org> - 1.0.0-3
- Ship complete Calamares module config and slideshow branding

* Sat Jun 27 2026 Kenny Glowner <sisyphuscode@fedoraproject.org> - 1.0.0-2
- Add installer-enabled flag for live USB Calamares auto-launch

* Fri Jun 26 2026 Kenny Glowner <sisyphuscode@fedoraproject.org> - 1.0.0-1
- Initial packaging of Sisyphus Calamares configuration
- Added official Sisyphus logo and branding descriptor
- Configured systemd-free module execution paths