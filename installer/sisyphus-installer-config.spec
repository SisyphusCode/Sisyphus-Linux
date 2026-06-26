Name:           sisyphus-installer-config
Version:        1.0.0
Release:        1%{?dist}
Summary:        Calamares configuration and branding for Sisyphus Linux
License:        GPLv3
URL:            https://github.com/sisyphuscode/Sisyphus-Linux
Source0:        %{name}-%{version}.tar.gz
BuildArch:      noarch

Requires:       calamares

%description
Custom settings, branding, and module configurations for the Sisyphus Linux deployment.
Includes systemd-free module execution paths and the official Sisyphus logo.

%prep
%autosetup -c

%build
# Nothing to compile

%install
rm -rf %{buildroot}

mkdir -p %{buildroot}/etc/calamares/
mkdir -p %{buildroot}/usr/share/calamares/branding/sisyphus/

install -p -m 644 installer/settings.conf %{buildroot}/etc/calamares/
install -p -m 644 installer/branding/sisyphus/branding.desc %{buildroot}/usr/share/calamares/branding/sisyphus/
install -p -m 644 installer/branding/sisyphus/logo.png %{buildroot}/usr/share/calamares/branding/sisyphus/

%files
%defattr(-,root,root,-)
%dir /usr/share/calamares/branding/sisyphus
/etc/calamares/settings.conf
/usr/share/calamares/branding/sisyphus/branding.desc
/usr/share/calamares/branding/sisyphus/logo.png

%changelog
* Fri Jun 26 2026 Kenny Glowner <sisyphuscode@fedoraproject.org> - 1.0.0-1
- Initial packaging of Sisyphus Calamares configuration
- Added official Sisyphus logo and branding descriptor
- Configured systemd-free module execution paths