use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;
use std::sync::OnceLock;

use gettextrs::{
    bind_textdomain_codeset,
    bindtextdomain,
    gettext,
    ngettext,
    setlocale,
    textdomain,
    LocaleCategory,
};

use crate::APP_NAME;

const GETTEXT_PACKAGE: &str = "galaxybook-camera";
const SYSTEM_LOCALE_DIR: &str = "/usr/share/locale";

const SNAPSHOT_DESKTOP_CANDIDATES: [&str; 4] = [
    "/usr/share/applications/org.gnome.Snapshot.desktop",
    "/usr/local/share/applications/org.gnome.Snapshot.desktop",
    "/var/lib/flatpak/exports/share/applications/org.gnome.Snapshot.desktop",
    ".local/share/flatpak/exports/share/applications/org.gnome.Snapshot.desktop",
];

static SNAPSHOT_CAMERA_TRANSLATIONS: OnceLock<HashMap<String, String>> = OnceLock::new();

pub fn init_i18n() {
    let _ = setlocale(LocaleCategory::LcAll, "");
    let _ = bindtextdomain(GETTEXT_PACKAGE, SYSTEM_LOCALE_DIR);
    let _ = bind_textdomain_codeset(GETTEXT_PACKAGE, "UTF-8");
    let _ = textdomain(GETTEXT_PACKAGE);
}

pub fn tr(message: &str) -> String {
    gettext(message)
}

pub fn trn(singular: &str, plural: &str, value: u32) -> String {
    ngettext(singular, plural, value)
}

pub fn trf(message: &str, replacements: &[(&str, String)]) -> String {
    let mut translated = gettext(message);
    for (key, value) in replacements {
        translated = translated.replace(&format!("{{{key}}}"), value);
    }
    translated
}

pub(crate) fn first_locale_candidate(locale_value: &str) -> Option<&str> {
    locale_value
        .split(':')
        .map(str::trim)
        .find(|candidate| !candidate.is_empty() && *candidate != "C" && *candidate != "POSIX")
}

fn locale_language_and_region(locale: &str) -> (String, Option<String>) {
    let normalized = locale
        .split(['.', '@'])
        .next()
        .unwrap_or(locale)
        .replace('-', "_");
    let mut parts = normalized.split('_');
    let language = parts.next().unwrap_or_default().to_ascii_lowercase();
    let region = parts.next().map(|value| value.to_ascii_lowercase());
    (language, region)
}

fn parse_snapshot_name_translations(contents: &str) -> HashMap<String, String> {
    let mut translations = HashMap::new();

    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key == "Name" {
            translations.insert(String::new(), value.to_string());
            continue;
        }
        if let Some(locale) = key
            .strip_prefix("Name[")
            .and_then(|value| value.strip_suffix(']'))
        {
            translations.insert(locale.to_ascii_lowercase(), value.to_string());
        }
    }

    translations
}

fn snapshot_camera_translations() -> &'static HashMap<String, String> {
    SNAPSHOT_CAMERA_TRANSLATIONS.get_or_init(|| {
        for candidate in SNAPSHOT_DESKTOP_CANDIDATES {
            let path = if candidate.starts_with('/') {
                Path::new(candidate).to_path_buf()
            } else if let Some(home_dir) = env::var_os("HOME") {
                Path::new(&home_dir).join(candidate)
            } else {
                continue;
            };

            let Ok(contents) = fs::read_to_string(&path) else {
                continue;
            };
            let translations = parse_snapshot_name_translations(&contents);
            if !translations.is_empty() {
                return translations;
            }
        }

        HashMap::new()
    })
}

fn translated_name_from_map(map: &HashMap<String, String>, locale: &str) -> Option<String> {
    if map.is_empty() {
        return None;
    }

    let (language, region) = locale_language_and_region(locale);
    if language.is_empty() {
        return map.get("").cloned();
    }

    if let Some(region) = region {
        let locale_key = format!("{language}_{region}");
        if let Some(value) = map.get(&locale_key) {
            return Some(value.clone());
        }
    }

    map.get(&language)
        .cloned()
        .or_else(|| map.get("").cloned())
}

fn fallback_camera_word_for_locale(locale: &str) -> &'static str {
    let (language, region) = locale_language_and_region(locale);
    match (language.as_str(), region.as_deref()) {
        ("pt", _) => "Câmera",
        ("es", _) => "Cámara",
        ("fr", _) => "Caméra",
        ("ru" | "uk", _) => "Камера",
        ("it", _) => "Fotocamera",
        ("de", _) => "Kamera",
        ("ja", _) => "カメラ",
        ("ko", _) => "카메라",
        ("zh", Some("tw" | "hk" | "mo")) => "相機",
        ("zh", _) => "相机",
        _ => "Camera",
    }
}

pub fn localized_camera_word_for_locale(locale: &str) -> String {
    translated_name_from_map(snapshot_camera_translations(), locale)
        .unwrap_or_else(|| fallback_camera_word_for_locale(locale).to_string())
}

pub fn localized_app_name_for_locale(locale: &str) -> String {
    format!("Galaxy Book {}", localized_camera_word_for_locale(locale))
}

pub fn localized_app_name() -> String {
    for key in ["LC_MESSAGES", "LC_ALL", "LANGUAGE", "LANG"] {
        if let Ok(value) = env::var(key) {
            if let Some(locale) = first_locale_candidate(&value) {
                return localized_app_name_for_locale(locale);
            }
        }
    }

    APP_NAME.to_string()
}
