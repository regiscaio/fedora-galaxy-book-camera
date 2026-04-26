use std::process::{Command, Stdio};

use crate::{tr, trf};

const DNF_COMMAND: &str = "/usr/bin/dnf";

fn command_output_text(stdout: &[u8], stderr: &[u8]) -> String {
    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(stdout));
    text.push_str(&String::from_utf8_lossy(stderr));
    text
}

fn fallback_command_error(output: String, fallback: &str) -> String {
    if output.trim().is_empty() {
        fallback.to_string()
    } else {
        output
    }
}

fn installed_package_names(packages: &[&str]) -> Vec<String> {
    packages
        .iter()
        .filter(|package| {
            Command::new("rpm")
                .args(["-q", *package])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        })
        .map(|package| (*package).to_string())
        .collect()
}

pub fn parse_dnf_check_update_package_names(output: &str, packages: &[&str]) -> Vec<String> {
    let mut found = Vec::new();

    for line in output.lines() {
        let first_column = line.split_whitespace().next().unwrap_or_default();
        if first_column.is_empty() {
            continue;
        }

        for package in packages {
            let exact_match = first_column == *package;
            let arch_qualified_match = first_column
                .strip_prefix(*package)
                .is_some_and(|suffix| suffix.starts_with('.'));

            if (exact_match || arch_qualified_match) && !found.iter().any(|name| name == *package) {
                found.push((*package).to_string());
            }
        }
    }

    found
}

pub fn package_update_names(packages: &[&str]) -> Result<Vec<String>, String> {
    let installed_packages = installed_package_names(packages);
    if installed_packages.is_empty() {
        return Ok(Vec::new());
    }
    let installed_package_refs = installed_packages
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();

    let output = Command::new(DNF_COMMAND)
        .arg("-q")
        .arg("check-update")
        .args(&installed_package_refs)
        .output()
        .map_err(|error| {
            trf(
                "Falha ao consultar atualizações via DNF: {error}",
                &[("error", error.to_string())],
            )
        })?;
    let output_text = command_output_text(&output.stdout, &output.stderr);

    match output.status.code() {
        Some(0) => Ok(Vec::new()),
        Some(100) => Ok(parse_dnf_check_update_package_names(
            &output_text,
            &installed_package_refs,
        )),
        _ => Err(fallback_command_error(
            output_text,
            &tr("O DNF não retornou detalhes sobre a consulta de atualizações."),
        )),
    }
}

pub fn install_package_updates(packages: &[&str]) -> Result<String, String> {
    let installed_packages = installed_package_names(packages);
    if installed_packages.is_empty() {
        return Ok(String::new());
    }
    let installed_package_refs = installed_packages
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();

    let output = Command::new("pkexec")
        .arg(DNF_COMMAND)
        .arg("upgrade")
        .arg("-y")
        .args(&installed_package_refs)
        .output()
        .map_err(|error| {
            trf(
                "Falha ao iniciar a atualização com privilégios administrativos: {error}",
                &[("error", error.to_string())],
            )
        })?;
    let output_text = command_output_text(&output.stdout, &output.stderr);

    if output.status.success() {
        Ok(output_text)
    } else {
        Err(fallback_command_error(
            output_text,
            &tr("A atualização falhou, mas não retornou saída textual."),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dnf4_check_update_lines() {
        let output = "\
galaxybook-camera.x86_64 1.1.0-1.fc44 caioregis
akmod-galaxybook-ov02c10.x86_64 1.1.0-1.fc44 caioregis
unrelated.x86_64 1.0-1 fedora
";
        assert_eq!(
            parse_dnf_check_update_package_names(
                output,
                &[
                    "galaxybook-camera",
                    "akmod-galaxybook-ov02c10",
                    "galaxybook-ov02c10-kmod-common",
                ],
            ),
            vec!["galaxybook-camera", "akmod-galaxybook-ov02c10"]
        );
    }

    #[test]
    fn parses_dnf5_table_lines() {
        let output = "\
Updating and loading repositories:
Repositories loaded.
Available upgrades
Name                         Arch   Version       Repository
galaxybook-ov02c10-kmod-common noarch 1.1.0-1.fc44 caioregis
galaxybook-camera            x86_64 1.1.0-1.fc44 caioregis
";
        assert_eq!(
            parse_dnf_check_update_package_names(
                output,
                &[
                    "galaxybook-camera",
                    "akmod-galaxybook-ov02c10",
                    "galaxybook-ov02c10-kmod-common",
                ],
            ),
            vec!["galaxybook-ov02c10-kmod-common", "galaxybook-camera"]
        );
    }
}
