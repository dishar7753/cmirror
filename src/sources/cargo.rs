use crate::config;
use crate::error::{MirrorError, Result};
use crate::traits::SourceManager;
use crate::types::Mirror;
use crate::utils;
use async_trait::async_trait;
use directories::BaseDirs;
use std::path::PathBuf;
use tokio::fs;

pub struct CargoManager {
    custom_path: Option<PathBuf>,
}

impl CargoManager {
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
impl SourceManager for CargoManager {
    fn name(&self) -> &'static str {
        "cargo"
    }

    fn requires_sudo(&self) -> bool {
        false
    }

    fn list_candidates(&self) -> Vec<Mirror> {
        config::get_candidates("cargo")
    }

    fn config_path(&self) -> PathBuf {
        if let Some(ref path) = self.custom_path {
            return path.clone();
        }
        BaseDirs::new()
            .map(|dirs| dirs.home_dir().join(".cargo").join("config.toml"))
            .unwrap_or_else(|| PathBuf::from(".").join(".cargo").join("config.toml"))
    }

    async fn current_url(&self) -> Result<Option<String>> {
        let path = self.config_path();
        if !fs::try_exists(&path).await.unwrap_or(false) {
            return Ok(None);
        }

        let content = fs::read_to_string(&path).await?;
        let config: toml::Value =
            toml::from_str(&content).unwrap_or(toml::Value::Table(toml::map::Map::new()));

        // Check [source.crates-io] replace-with
        if let Some(replace_with) = config
            .get("source")
            .and_then(|s| s.get("crates-io"))
            .and_then(|c| c.get("replace-with"))
            .and_then(|v| v.as_str())
        {
            // If replace-with is set (e.g., "mirror"), look up [source.mirror]
            if let Some(registry) = config
                .get("source")
                .and_then(|s| s.get(replace_with))
                .and_then(|m| m.get("registry"))
                .and_then(|r| r.as_str())
            {
                return Ok(Some(registry.to_string()));
            }
        }

        Ok(None)
    }

    async fn set_source(&self, mirror: &Mirror) -> Result<()> {
        let path = self.config_path();

        // 1. Ensure .cargo directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        // 2. Read existing TOML or create empty
        let content = if fs::try_exists(&path).await.unwrap_or(false) {
            fs::read_to_string(&path).await?
        } else {
            String::new()
        };

        // 3. Backup
        if !content.is_empty() {
            utils::backup_file(&path).await?;
        }

        // 4. Update TOML
        let mut config: toml::Value =
            toml::from_str(&content).unwrap_or(toml::Value::Table(toml::map::Map::new()));

        // We need to modify the table structure.
        // Since `toml` crate Value is not easily mutable deeply, let's reconstruct parts of it.
        // Or simpler: Parse as Table, modify, serialize.

        let root = config.as_table_mut().ok_or(MirrorError::Custom(
            "Invalid config.toml format".to_string(),
        ))?;

        // Ensure [source] table exists
        let source_entry = root
            .entry("source")
            .or_insert(toml::Value::Table(toml::map::Map::new()));
        let source_table = source_entry
            .as_table_mut()
            .ok_or(MirrorError::Custom("Invalid [source] section".to_string()))?;

        // 4.1 Set [source.crates-io] replace-with = 'mirror'
        let crates_io_entry = source_table
            .entry("crates-io")
            .or_insert(toml::Value::Table(toml::map::Map::new()));
        let crates_io_table = crates_io_entry.as_table_mut().ok_or(MirrorError::Custom(
            "Invalid [source.crates-io] section".to_string(),
        ))?;

        // Use a generic name 'mirror' for the replacement source
        crates_io_table.insert(
            "replace-with".to_string(),
            toml::Value::String("mirror".to_string()),
        );

        // 4.2 Set [source.mirror] registry = "..."
        let mirror_entry = source_table
            .entry("mirror")
            .or_insert(toml::Value::Table(toml::map::Map::new()));
        let mirror_table = mirror_entry.as_table_mut().ok_or(MirrorError::Custom(
            "Invalid [source.mirror] section".to_string(),
        ))?;

        mirror_table.insert(
            "registry".to_string(),
            toml::Value::String(mirror.url.clone()),
        );

        // 5. Write back
        let new_content = toml::to_string_pretty(&config)?;
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
    async fn test_cargo_flow() -> Result<()> {
        let dir = tempdir()?;
        let config_path = dir.path().join("config.toml");
        let manager = CargoManager::with_path(config_path.clone());

        // 1. Initial state
        assert!(manager.current_url().await?.is_none());

        // 2. Set source
        let mirror = Mirror {
            name: "TestCargo".to_string(),
            url: "sparse+https://test.crates.io/index".to_string(),
        };
        manager.set_source(&mirror).await?;

        // 3. Check current
        assert_eq!(manager.current_url().await?, Some(mirror.url.clone()));

        // Check TOML structure
        let content = fs::read_to_string(&config_path).await?;
        assert!(content.contains("[source.crates-io]"));
        assert!(content.contains("replace-with = \"mirror\""));
        assert!(content.contains("[source.mirror]"));
        assert!(content.contains(&format!("registry = \"{}\"", mirror.url)));

        // 4. Set another
        let mirror2 = Mirror {
            name: "TestCargo2".to_string(),
            url: "sparse+https://test2.crates.io/index".to_string(),
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
