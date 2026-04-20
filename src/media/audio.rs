use std::process::Command;

#[derive(Clone)]
pub struct AudioSourceOption {
    pub id: String,
    pub label: String,
}

pub fn detect_audio_sources() -> Vec<AudioSourceOption> {
    let output = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-sources")
        .arg("pulse")
        .output();

    let Ok(output) = output else {
        return default_audio_sources();
    };

    if !output.status.success() && output.stdout.is_empty() && output.stderr.is_empty() {
        return default_audio_sources();
    }

    let mut combined = String::from_utf8_lossy(&output.stdout).to_string();
    if !output.stderr.is_empty() {
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(&String::from_utf8_lossy(&output.stderr));
    }

    parse_audio_sources(&combined)
}

fn default_audio_sources() -> Vec<AudioSourceOption> {
    vec![AudioSourceOption {
        id: "default".to_string(),
        label: "Padrao do sistema".to_string(),
    }]
}

fn parse_audio_sources(raw: &str) -> Vec<AudioSourceOption> {
    let mut sources = default_audio_sources();

    for line in raw.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with("Auto-detected sources for ") {
            continue;
        }

        let is_default = trimmed.starts_with('*');
        let entry = trimmed.trim_start_matches('*').trim();
        let Some((id, rest)) = entry.split_once(' ') else {
            continue;
        };
        if id.ends_with(".monitor") {
            continue;
        }

        let label = if let (Some(start), Some(end)) = (rest.find('['), rest.rfind(']')) {
            let text = rest[(start + 1)..end].trim();
            if is_default {
                format!("{text} (padrao atual)")
            } else {
                text.to_string()
            }
        } else if is_default {
            format!("{id} (padrao atual)")
        } else {
            id.to_string()
        };

        if !sources.iter().any(|source| source.id == id) {
            sources.push(AudioSourceOption {
                id: id.to_string(),
                label,
            });
        }
    }

    sources
}

pub fn selected_audio_source_label(
    options: &[AudioSourceOption],
    selected_id: &str,
) -> String {
    options
        .iter()
        .find(|option| option.id == selected_id)
        .map(|option| option.label.clone())
        .unwrap_or_else(|| selected_id.to_string())
}
