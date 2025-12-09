use crate::config;
use crate::error::Result;
use crate::traits::SourceManager;
use crate::types::Mirror;
use crate::utils;
use async_trait::async_trait;
use directories::BaseDirs;
use regex::Regex;
use std::path::PathBuf;
use tokio::fs;

pub struct NpmManager {
    custom_path: Option<PathBuf>,
}

impl NpmManager {
    pub fn new() -> Self {
        Self { custom_path: None }
    }

    #[cfg(test)]
    pub fn with_path(path: PathBuf) -> Self {
        Self {
            custom_path: Some(path),
        }
    }
}

#[async_trait]
impl SourceManager for NpmManager {
    fn name(&self) -> &'static str {
        "npm"
    }

    fn requires_sudo(&self) -> bool {
        false
    }

    fn list_candidates(&self) -> Vec<Mirror> {
        config::get_candidates("npm")
    }

    fn config_path(&self) -> PathBuf {
        if let Some(ref path) = self.custom_path {
            return path.clone();
        }
        BaseDirs::new()
            .map(|dirs| dirs.home_dir().join(".npmrc"))
            .unwrap_or_else(|| PathBuf::from(".").join(".npmrc"))
    }

    async fn current_url(&self) -> Result<Option<String>> {
        let path = self.config_path();
        if !fs::try_exists(&path).await.unwrap_or(false) {
            return Ok(None);
        }

        let content = fs::read_to_string(&path).await?;

        // Match "registry=https://..."
        let re = Regex::new(r"(?m)^registry\s*=\s*(.+)$")?;

        if let Some(caps) = re.captures(&content) {
            Ok(Some(caps[1].trim().to_string()))
        } else {
            Ok(None)
        }
    }

    async fn set_source(&self, mirror: &Mirror) -> Result<()> {
        let path = self.config_path();

        // 1. Ensure directory exists (though home usually exists)
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        // 2. Read existing content
        let content = if fs::try_exists(&path).await.unwrap_or(false) {
            fs::read_to_string(&path).await?
        } else {
            String::new()
        };

        // 3. Backup using generic utility
        if !content.is_empty() {
            utils::backup_file(&path).await?;
        }

        // 4. Update content
        let new_line = format!("registry={}", mirror.url);
        let re = Regex::new(r"(?m)^registry\s*=\s*.*$")?;

        let new_content = if re.is_match(&content) {
            re.replace(&content, new_line.as_str()).to_string()
        } else {
            let prefix = if content.is_empty() || content.ends_with('\n') {
                ""
            } else {
                "\n"
            };
            format!("{}{}{}\n", content, prefix, new_line)
        };

        // 5. Write
        fs::write(&path, new_content).await?;
        Ok(())
    }

    async fn restore(&self) -> Result<()> {
        utils::restore_latest_backup(&self.config_path()).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_npm_flow() -> Result<()> {
        let dir = tempdir()?;
        let config_path = dir.path().join(".npmrc");
        let manager = NpmManager::with_path(config_path.clone());

        // 1. Initial state
        assert!(manager.current_url().await?.is_none());

        // 2. Set source
        let mirror = Mirror {
            name: "TestNpm".to_string(),
            url: "https://registry.npm.test.org/".to_string(),
        };
        manager.set_source(&mirror).await?;

        // 3. Check current
        assert_eq!(manager.current_url().await?, Some(mirror.url.clone()));

        let content = fs::read_to_string(&config_path).await?;
        assert!(content.contains(&format!("registry={}", mirror.url)));

        // 4. Set another
        let mirror2 = Mirror {
            name: "TestNpm2".to_string(),
            url: "https://registry.npm.test2.org/".to_string(),
        };
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        manager.set_source(&mirror2).await?;
        assert_eq!(manager.current_url().await?, Some(mirror2.url.clone()));

        // 5. Restore
        manager.restore().await?;
        assert_eq!(manager.current_url().await?, Some(mirror.url));

        Ok(())
    }
}
