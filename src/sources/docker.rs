use crate::config;
use crate::error::Result;
use crate::traits::SourceManager;
use crate::types::Mirror;
use crate::utils;
use async_trait::async_trait;
use directories::BaseDirs;
use serde_json::Value;
use std::path::PathBuf;
use tokio::fs;

pub struct DockerManager;

impl DockerManager {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl SourceManager for DockerManager {
    fn name(&self) -> &'static str {
        "docker"
    }

    fn requires_sudo(&self) -> bool {
        true
    }

    fn list_candidates(&self) -> Vec<Mirror> {
        config::get_candidates("docker")
    }

    fn config_path(&self) -> PathBuf {
        if cfg!(target_os = "windows") {
            PathBuf::from(r"C:\ProgramData\docker\config\daemon.json")
        } else if cfg!(target_os = "macos") {
            // Docker Desktop for Mac user config
            BaseDirs::new()
                .map(|dirs| dirs.home_dir().join(".docker").join("daemon.json"))
                .unwrap_or_else(|| PathBuf::from(".").join(".docker").join("daemon.json"))
        } else {
            // Linux and others
            PathBuf::from("/etc/docker/daemon.json")
        }
    }

    async fn current_url(&self) -> Result<Option<String>> {
        let path = self.config_path();
        if !fs::try_exists(&path).await.unwrap_or(false) {
            return Ok(None);
        }

        // 读取并解析 JSON
        let content = fs::read_to_string(&path).await?;

        let v: Value = serde_json::from_str(&content)?;

        // 提取 registry-mirrors 数组的第一个元素
        if let Some(mirrors) = v.get("registry-mirrors").and_then(|v| v.as_array()) {
            if let Some(first) = mirrors.first().and_then(|v| v.as_str()) {
                return Ok(Some(first.to_string()));
            }
        }

        Ok(None)
    }

    async fn set_source(&self, mirror: &Mirror) -> Result<()> {
        let path = self.config_path();

        // 1. 读取现有配置或创建空对象

        let mut config: Value = if fs::try_exists(&path).await.unwrap_or(false) {
            let content = fs::read_to_string(&path).await?;

            serde_json::from_str(&content).unwrap_or(serde_json::json!({}))
        } else {
            // 确保目录存在

            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).await?;
            }

            serde_json::json!({})
        };

        // 2. 备份

        utils::backup_file(&path).await?;

        // 3. 修改 registry-mirrors 字段

        // Docker 支持配置多个 mirror，但为了简单起见，我们把选中的置为唯一一个或第一个

        config["registry-mirrors"] = serde_json::json!([mirror.url]);

        // 4. 写入文件 (Pretty print 格式化)

        let new_content = serde_json::to_string_pretty(&config)?;

        fs::write(&path, new_content).await?;

        Ok(())
    }

    async fn restore(&self) -> Result<()> {
        utils::restore_latest_backup(&self.config_path()).await
    }
}
