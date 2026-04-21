%global app_id com.caioregis.GalaxyBookCamera

Name:           galaxybook-camera
Version:        1.0.0
Release:        3%{?dist}
Summary:        Native libcamera camera app for Galaxy Book on Fedora

License:        GPL-2.0-only
URL:            https://github.com/regiscaio/fedora-galaxy-book-camera
Source0:        %{name}-%{version}.tar.gz

ExclusiveArch:  x86_64

BuildRequires:  cargo
BuildRequires:  clang
BuildRequires:  desktop-file-utils
BuildRequires:  gettext
BuildRequires:  gcc-c++
BuildRequires:  make
BuildRequires:  pkgconfig(gtk4)
BuildRequires:  pkgconfig(libadwaita-1)
BuildRequires:  pkgconfig(libcamera)
BuildRequires:  rust

Requires:       (ffmpeg-free or ffmpeg)
Requires:       akmod-galaxybook-ov02c10 >= 1.0.0

%description
Galaxy Book Camera is a native GTK4 and libadwaita camera app for Fedora on
Galaxy Book notebooks. It uses libcamera directly and provides embedded
preview, photo capture, video recording, and manual image tuning controls.

%prep
%autosetup -n %{name}-%{version}

%build
cargo --offline build --release --locked --bin galaxybook-camera

%install
install -Dm755 target/release/galaxybook-camera %{buildroot}%{_bindir}/galaxybook-camera
install -Dm644 assets/galaxybook-camera.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/%{app_id}.svg
install -Dm644 assets/camera-timer-symbolic.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/actions/camera-timer-symbolic.svg
install -Dm644 data/libcamera/simple/ov02c10.yaml %{buildroot}%{_datadir}/galaxybook-camera/libcamera/simple/ov02c10.yaml
for lang in en es it; do \
  install -d %{buildroot}%{_datadir}/locale/${lang}/LC_MESSAGES; \
  msgfmt po/${lang}.po -o %{buildroot}%{_datadir}/locale/${lang}/LC_MESSAGES/%{name}.mo; \
done
sed \
  -e 's|@EXEC@|galaxybook-camera|g' \
  -e 's|@ICON@|%{app_id}|g' \
  -e 's|@STARTUP_WM_CLASS@|%{app_id}|g' \
  data/%{app_id}.desktop > %{app_id}.desktop
install -Dm644 %{app_id}.desktop %{buildroot}%{_datadir}/applications/%{app_id}.desktop
install -Dm644 data/%{app_id}.metainfo.xml %{buildroot}%{_datadir}/metainfo/%{app_id}.metainfo.xml

%check
desktop-file-validate %{app_id}.desktop
cargo --offline test --locked --lib --bin galaxybook-camera

%files
%license LICENSE
%{_bindir}/galaxybook-camera
%{_datadir}/applications/%{app_id}.desktop
%{_datadir}/icons/hicolor/scalable/apps/%{app_id}.svg
%{_datadir}/icons/hicolor/scalable/actions/camera-timer-symbolic.svg
%{_datadir}/galaxybook-camera/libcamera/simple/ov02c10.yaml
%{_datadir}/locale/en/LC_MESSAGES/%{name}.mo
%{_datadir}/locale/es/LC_MESSAGES/%{name}.mo
%{_datadir}/locale/it/LC_MESSAGES/%{name}.mo
%{_datadir}/metainfo/%{app_id}.metainfo.xml

%changelog
* Mon Apr 20 2026 Caio Régis <regiscaio@users.noreply.github.com> - 1.0.0-3
- Add an explicit GPL-2.0-only license to the project and package metadata
- Add multilingual README variants and language navigation links

* Mon Apr 20 2026 Caio Régis <regiscaio@users.noreply.github.com> - 1.0.0-2
- Improve color neutrality in deep shadows and highlight extremes

* Sun Apr 19 2026 Caio Régis <regiscaio@users.noreply.github.com> - 1.0.0-1
- Start the stable RPM line at 1.0.0
