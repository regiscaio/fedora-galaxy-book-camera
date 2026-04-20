use std::env;
use std::path::PathBuf;
use std::process::Command;

use chrono::Local;

const CONFIG_PATH_RELATIVE: &str = ".config/camera-tuned.env";
const CAMERA_LIBRARY_DIRNAME: &str = "Camera";

pub(crate) fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

pub(crate) fn default_user_dir(kind: &str) -> Option<PathBuf> {
    let env_name = match kind {
        "PICTURES" => "XDG_PICTURES_DIR",
        "VIDEOS" => "XDG_VIDEOS_DIR",
        _ => return None,
    };

    if let Some(path) = env::var_os(env_name) {
        let path = PathBuf::from(path);
        if !path.as_os_str().is_empty() {
            return Some(path);
        }
    }

    let output = Command::new("xdg-user-dir").arg(kind).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

pub(crate) fn cache_dir() -> PathBuf {
    env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".cache"))
}

pub fn photo_library_dir() -> PathBuf {
    default_user_dir("PICTURES")
        .unwrap_or_else(|| home_dir().join("Pictures"))
        .join(CAMERA_LIBRARY_DIRNAME)
}

pub fn video_library_dir() -> PathBuf {
    default_user_dir("VIDEOS")
        .unwrap_or_else(|| home_dir().join("Videos"))
        .join(CAMERA_LIBRARY_DIRNAME)
}

pub fn timestamp() -> String {
    Local::now().format("%Y%m%d-%H%M%S").to_string()
}

pub fn default_config_path() -> PathBuf {
    home_dir().join(CONFIG_PATH_RELATIVE)
}
