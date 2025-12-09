use crate::config;
use crate::error::{MirrorError, Result};
use crate::traits::SourceManager;
use crate::types::Mirror;
use async_trait::async_trait;
use std::path::PathBuf;
use tokio::process::Command;

pub struct GoManager;

impl GoManager {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl SourceManager for GoManager {
    fn name(&self) -> &'static str {
        "go"
    }

    fn requires_sudo(&self) -> bool {
        false
    }

    fn list_candidates(&self) -> Vec<Mirror> {
        config::get_candidates("go")
    }

    fn config_path(&self) -> PathBuf {
        // Go configuration is managed via the 'go env' command,
        // which writes to a platform-specific file (e.g., ~/.config/go/env).
        // Returning a placeholder or trying to resolve `go env GOENV`.
        PathBuf::from("GO111MODULE/GOPROXY")
    }

    async fn current_url(&self) -> Result<Option<String>> {
        // Use `go env GOPROXY` to get the current value
        let output = Command::new("go").args(["env", "GOPROXY"]).output().await;

        match output {
            Ok(o) if o.status.success() => {
                let stdout = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if stdout.is_empty() {
                    Ok(None)
                } else {
                    // Usually returns "https://proxy.golang.org,direct"
                    // We might want to split by comma and take the first one?
                    let first = stdout.split(',').next().unwrap_or(&stdout).to_string();
                    Ok(Some(first))
                }
            }
            _ => Ok(None), // Go not installed or error
        }
    }

    async fn set_source(&self, mirror: &Mirror) -> Result<()> {
        // Use `go env -w GOPROXY=...`
        // Append ",direct" to ensure fallback works for private modules
        let new_val = format!("{},direct", mirror.url);

        let status = Command::new("go")
            .args(["env", "-w", &format!("GOPROXY={}", new_val)])
            .status()
            .await?;

        if !status.success() {
            return Err(MirrorError::Custom(
                "Failed to set GOPROXY via 'go env -w'".to_string(),
            ));
        }

        Ok(())
    }

    async fn restore(&self) -> Result<()> {
        println!("Restoring GOPROXY to default (unsetting)...");
        let status = Command::new("go")
            .args(["env", "-u", "GOPROXY"])
            .status()
            .await?;

        if !status.success() {
            return Err(MirrorError::Custom(
                "Failed to unset GOPROXY via 'go env -u'".to_string(),
            ));
        }
        Ok(())
    }
}
