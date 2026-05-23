use anyhow::Result;
use serde::Serialize;

use iota_core::acp;
use iota_core::config::{self, NimiaConfig};
use iota_core::daemon;

#[derive(Serialize)]
struct CombinedInfo {
    config_path: String,
    daemon_addr: String,
    backends: Vec<BackendInfo>,
}

#[derive(Serialize)]
struct BackendInfo {
    backend: String,
    enabled: bool,
    check_status: String,
    acp_command: String,
    version_mapping: BackendVersionInfo,
    model: String,
}

#[derive(Serialize)]
struct BackendVersionInfo {
    acp: Option<String>,
    bin: Option<String>,
}

pub(super) fn print_combined_info(config: &NimiaConfig) -> Result<()> {
    let info = CombinedInfo {
        config_path: config::config_path()?.display().to_string(),
        daemon_addr: daemon::daemon_addr(),
        backends: acp::ALL_BACKENDS
            .iter()
            .copied()
            .map(|backend| backend_info(config, backend))
            .collect(),
    };
    println!("{}", serde_json::to_string_pretty(&info)?);
    Ok(())
}

fn backend_info(config: &NimiaConfig, backend: acp::AcpBackend) -> BackendInfo {
    let section = config::backend_config(config, backend);
    let check_status = match section {
        Some(section) if !section.enabled => "disabled",
        Some(section)
            if section
                .acp
                .as_ref()
                .is_some_and(|acp| !acp.command.trim().is_empty()) =>
        {
            "configured"
        }
        Some(_) => "missing acp.command",
        None => "missing section",
    };
    let enabled = section.is_some_and(|section| section.enabled);
    let acp_command = section
        .and_then(|section| section.acp.as_ref())
        .map(config::command_label)
        .unwrap_or_else(|| "missing acp".to_string());
    let model = section
        .map(config::configured_model)
        .unwrap_or(None)
        .unwrap_or_else(|| "-".to_string());
    let version_mapping = backend_version_info(section, backend);

    BackendInfo {
        backend: backend.to_string(),
        enabled,
        check_status: check_status.to_string(),
        acp_command,
        version_mapping,
        model,
    }
}

fn backend_version_info(
    section: Option<&config::BackendConfig>,
    backend: acp::AcpBackend,
) -> BackendVersionInfo {
    let explicit = section.and_then(|section| section.version_mapping.as_ref());
    let acp = explicit
        .and_then(|mapping| non_empty_string(mapping.acp.as_ref()))
        .or_else(|| section.and_then(inferred_acp_version_spec));
    let bin = explicit
        .and_then(|mapping| non_empty_string(mapping.bin.as_ref()))
        .or_else(|| section.and_then(|section| inferred_bin_version_spec(backend, section)));

    BackendVersionInfo { acp, bin }
}

fn non_empty_string(value: Option<&String>) -> Option<String> {
    value
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn inferred_acp_version_spec(section: &config::BackendConfig) -> Option<String> {
    section
        .acp
        .as_ref()
        .and_then(npm_package_spec)
        .and_then(|package| package_version(&package))
}

fn inferred_bin_version_spec(
    backend: acp::AcpBackend,
    section: &config::BackendConfig,
) -> Option<String> {
    if backend == acp::AcpBackend::Codex {
        return None;
    }
    let package = section.acp.as_ref().and_then(npm_package_spec);
    package.and_then(|package| package_version(&package))
}

fn npm_package_spec(command: &config::CommandConfig) -> Option<String> {
    command
        .args
        .iter()
        .find(|arg| !arg.starts_with('-') && arg.contains('@'))
        .cloned()
}

fn package_version(package: &str) -> Option<String> {
    let (_, version) = package.rsplit_once('@')?;
    let version = version.trim();
    if version.is_empty() || version == "latest" {
        return None;
    }
    Some(version.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_info_includes_version_mapping() {
        let config = NimiaConfig {
            codex: Some(config::BackendConfig {
                enabled: true,
                acp: Some(config::CommandConfig {
                    command: "npx".to_string(),
                    args: vec![
                        "-y".to_string(),
                        "@zed-industries/codex-acp@0.12.0".to_string(),
                    ],
                }),
                version_mapping: Some(config::BackendVersionMapping {
                    acp: Some("0.12.0".to_string()),
                    bin: Some("0.128.0".to_string()),
                }),
                ..config::BackendConfig::default()
            }),
            ..NimiaConfig::default()
        };

        let value = serde_json::to_value(backend_info(&config, acp::AcpBackend::Codex)).unwrap();

        assert_eq!(value["version_mapping"]["acp"], "0.12.0");
        assert_eq!(value["version_mapping"]["bin"], "0.128.0");
    }

    #[test]
    fn backend_info_does_not_infer_codex_bin_from_acp_adapter() {
        let config = NimiaConfig {
            codex: Some(config::BackendConfig {
                enabled: true,
                acp: Some(config::CommandConfig {
                    command: "npx".to_string(),
                    args: vec![
                        "-y".to_string(),
                        "@zed-industries/codex-acp@0.12.0".to_string(),
                    ],
                }),
                ..config::BackendConfig::default()
            }),
            ..NimiaConfig::default()
        };

        let value = serde_json::to_value(backend_info(&config, acp::AcpBackend::Codex)).unwrap();

        assert_eq!(value["version_mapping"]["acp"], "0.12.0");
        assert!(value["version_mapping"]["bin"].is_null());
    }
}
