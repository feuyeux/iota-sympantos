use anyhow::{Context, Result, bail};
use std::collections::{BTreeMap, btree_map::Entry};
use std::path::PathBuf;

use crate::acp::{self, AcpBackend, AcpClient};
use crate::config::{
    NimiaConfig, backend_config, backend_process_env, config_path, normalized_acp_command,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ClientKey {
    backend: AcpBackend,
    cwd: PathBuf,
}

pub struct IotaEngine {
    config: NimiaConfig,
    clients: BTreeMap<ClientKey, AcpClient>,
    show_native: bool,
    timeout_ms: u64,
}

impl IotaEngine {
    pub fn new(config: NimiaConfig, _cwd: PathBuf, show_native: bool, timeout_ms: u64) -> Self {
        Self {
            config,
            clients: BTreeMap::new(),
            show_native,
            timeout_ms,
        }
    }

    pub async fn warm_enabled_backends_in_cwd(&mut self, cwd: PathBuf) -> Result<usize> {
        let mut handles = Vec::new();
        for backend in acp::ALL_BACKENDS {
            let key = ClientKey {
                backend,
                cwd: cwd.clone(),
            };
            if self.clients.contains_key(&key) {
                continue;
            }
            let Some(section) = backend_config(&self.config, backend) else {
                continue;
            };
            if !section.enabled {
                continue;
            }
            let Some(acp_config) = section.acp.as_ref() else {
                eprintln!("Skipping {}: missing acp config", backend);
                continue;
            };
            if acp_config.command.trim().is_empty() {
                eprintln!("Skipping {}: missing acp.command", backend);
                continue;
            }

            let env = backend_process_env(backend, section);
            let command = normalized_acp_command(backend, section, acp_config);
            let cwd = cwd.clone();
            let show_native = self.show_native;
            let timeout_ms = self.timeout_ms;
            handles.push(tokio::spawn(async move {
                match AcpClient::start(
                    backend,
                    cwd.clone(),
                    env,
                    Some(command),
                    show_native,
                    timeout_ms,
                )
                .await
                {
                    Ok(client) => Some((ClientKey { backend, cwd }, client)),
                    Err(err) => {
                        eprintln!("Failed to warm {}: {}", backend, err);
                        None
                    }
                }
            }));
        }

        for handle in handles {
            if let Ok(Some((key, client))) = handle.await {
                self.clients.insert(key, client);
            }
        }
        Ok(self.clients.len())
    }

    pub async fn prompt_in_cwd(
        &mut self,
        backend: AcpBackend,
        cwd: PathBuf,
        prompt: &str,
    ) -> Result<String> {
        self.ensure_client(backend, cwd.clone()).await?;
        let key = ClientKey {
            backend,
            cwd: cwd.clone(),
        };
        self.clients
            .get_mut(&key)
            .context("ACP client missing after warm")?
            .prompt_with_cwd(&cwd, prompt)
            .await
    }

    pub fn is_warmed_in_cwd(&self, backend: AcpBackend, cwd: &PathBuf) -> bool {
        self.clients.contains_key(&ClientKey {
            backend,
            cwd: cwd.clone(),
        })
    }

    pub async fn shutdown(mut self) {
        while let Some((_, client)) = self.clients.pop_first() {
            client.shutdown().await;
        }
    }

    async fn ensure_client(&mut self, backend: AcpBackend, cwd: PathBuf) -> Result<()> {
        let key = ClientKey {
            backend,
            cwd: cwd.clone(),
        };
        if self.clients.contains_key(&key) {
            return Ok(());
        }
        let client = self.start_client(backend, cwd.clone()).await?;
        match self.clients.entry(key) {
            Entry::Vacant(entry) => {
                entry.insert(client);
            }
            Entry::Occupied(_) => {}
        }
        Ok(())
    }

    async fn start_client(&self, backend: AcpBackend, cwd: PathBuf) -> Result<AcpClient> {
        let path = config_path()?;
        let section = backend_config(&self.config, backend).with_context(|| {
            format!(
                "Missing backend section for {} in {}",
                backend,
                path.display()
            )
        })?;
        if !section.enabled {
            bail!("Backend {} is disabled in {}", backend, path.display());
        }
        let acp_config = section.acp.as_ref().with_context(|| {
            format!(
                "Missing acp config for backend {} in {}",
                backend,
                path.display()
            )
        })?;
        if acp_config.command.trim().is_empty() {
            bail!(
                "Missing acp.command for backend {} in {}",
                backend,
                path.display()
            );
        }

        AcpClient::start(
            backend,
            cwd,
            backend_process_env(backend, section),
            Some(normalized_acp_command(backend, section, acp_config)),
            self.show_native,
            self.timeout_ms,
        )
        .await
    }
}
