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

pub struct PipManager {
    custom_path: Option<PathBuf>,
}

impl PipManager {
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
impl SourceManager for PipManager {
    fn name(&self) -> &'static str {
        "pip"
    }

    fn requires_sudo(&self) -> bool {
        false
    }

    fn list_candidates(&self) -> Vec<Mirror> {
        config::get_candidates("pip")
    }

    fn config_path(&self) -> PathBuf {
        if let Some(ref path) = self.custom_path {
            return path.clone();
        }

        if let Some(base_dirs) = BaseDirs::new() {
            let config_dir = base_dirs.config_dir();
            if cfg!(target_os = "windows") {
                // Windows: %APPDATA%\pip\pip.ini
                config_dir.join("pip").join("pip.ini")
            } else {
                // Linux/macOS: ~/.config/pip/pip.conf (Standard XDG)
                // Note: macOS might also check ~/Library/Application Support/pip/pip.conf if config_dir maps there.
                config_dir.join("pip").join("pip.conf")
            }
        } else {
            // Fallback
            PathBuf::from(".").join("pip.conf")
        }
    }

    async fn current_url(&self) -> Result<Option<String>> {
        let path = self.config_path();
        if !fs::try_exists(&path).await.unwrap_or(false) {
            return Ok(None);
        }

        let content = fs::read_to_string(&path).await?;

        // 使用正则提取 index-url 的值
        // 支持 index-url = https://... 或 index-url=https://...
        let re = Regex::new(r"(?m)^index-url\s*=\s*(.+)$")?;

        if let Some(caps) = re.captures(&content) {
            // 提取第一个捕获组
            Ok(Some(caps[1].trim().to_string()))
        } else {
            Ok(None)
        }
    }

    async fn set_source(&self, mirror: &Mirror) -> Result<()> {
        let path = self.config_path();

        // 1. 确保目录存在
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        // 2. 读取旧内容或初始化空内容
        let content = if fs::try_exists(&path).await.unwrap_or(false) {
            fs::read_to_string(&path).await?
        } else {
            String::new()
        };

        // 3. 备份 (如果文件存在且不为空)
        if !content.is_empty() {
            utils::backup_file(&path).await?;
        }

        // 4. 构造新内容 (使用正则替换，保留其他配置)
        let new_url_line = format!("index-url = {}", mirror.url);
        let re = Regex::new(r"(?m)^index-url\s*=\s*.*$")?;

        let new_content = if re.is_match(&content) {
            // 情况 A: 已存在 index-url，直接替换该行
            re.replace(&content, new_url_line.as_str()).to_string()
        } else {
            // 情况 B: 不存在 index-url
            // 检查是否有 [global] 节
            if content.contains("[global]") {
                content.replace("[global]", &format!("[global]\n{}", new_url_line))
            } else {
                // 情况 C: 既没 key 也没 section，或者文件为空，追加全部
                let prefix = if content.is_empty() { "" } else { "\n" };
                format!("{}{}[global]\n{}\n", content, prefix, new_url_line)
            }
        };

        // 5. 写入
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
    async fn test_pip_flow() -> Result<()> {
        let dir = tempdir()?;
        let config_path = dir.path().join("pip.conf");
        let manager = PipManager::with_path(config_path.clone());

        // 1. Initial state: None
        assert!(manager.current_url().await?.is_none());

        // 2. Set source
        let mirror = Mirror {
            name: "Test".to_string(),
            url: "https://test.pypi.org/simple".to_string(),
        };
        manager.set_source(&mirror).await?;

        // 3. Check current url
        let current = manager.current_url().await?;
        assert_eq!(current, Some(mirror.url.clone()));

        // 4. Check file content
        let content = fs::read_to_string(&config_path).await?;
        assert!(content.contains("[global]"));
        assert!(content.contains(&format!("index-url = {}", mirror.url)));

        // 5. Set another source (Backup should be created)
        let mirror2 = Mirror {
            name: "Test2".to_string(),
            url: "https://test2.pypi.org/simple".to_string(),
        };
        // Sleep a bit to ensure timestamp diff if backup naming relies on second precision
        // (Our utils uses seconds, so we might overwrite backup if too fast?
        // utils::backup_file uses SystemTime::now()...as_secs(). If running super fast, timestamp might be same.
        // But backup_file appends timestamp. If same timestamp, it overwrites the backup.
        // Restore finds the latest backup. If we have only one (overwritten), it restores that.)
        // Let's explicitly sleep 1s to be safe or just proceed.
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        manager.set_source(&mirror2).await?;
        assert_eq!(manager.current_url().await?, Some(mirror2.url.clone()));

        // 6. Restore
        manager.restore().await?;
        // Should be back to mirror 1
        assert_eq!(manager.current_url().await?, Some(mirror.url));

        Ok(())
    }
}
