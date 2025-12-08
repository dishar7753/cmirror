use crate::traits::SourceManager;
use crate::types::Mirror;
use crate::error::{Result, MirrorError};
use crate::config;
use crate::utils;
use async_trait::async_trait;
use regex::Regex;
use std::path::PathBuf;
use tokio::fs;

pub struct AptManager {
    distro: String,
    custom_path: Option<PathBuf>,
}

impl AptManager {
    pub fn new() -> Self {
        // Simple heuristic detection (synchronous is fine here for construction, 
        // or we can detect lazily. For now, let's try to detect once).
        // Since we are inside a specific tool, we can try to read /etc/os-release
        let distro = Self::detect_distro().unwrap_or_else(|| "ubuntu".to_string());
        Self { 
            distro,
            custom_path: None 
        }
    }

    #[cfg(test)]
    pub fn with_distro_and_path(distro: String, path: PathBuf) -> Self {
        Self {
            distro,
            custom_path: Some(path),
        }
    }

    fn detect_distro() -> Option<String> {
        // Quick check of os-release
        if let Ok(content) = std::fs::read_to_string("/etc/os-release") {
            if content.to_lowercase().contains("id=ubuntu") {
                return Some("ubuntu".to_string());
            } else if content.to_lowercase().contains("id=debian") {
                return Some("debian".to_string());
            }
        }
        // Fallback: check file existence
        if std::path::Path::new("/etc/apt/sources.list").exists() {
             // Maybe try to guess from content?
             if let Ok(c) = std::fs::read_to_string("/etc/apt/sources.list") {
                 if c.contains("ubuntu") { return Some("ubuntu".to_string()); }
                 if c.contains("debian") { return Some("debian".to_string()); }
             }
        }
        
        // Default to ubuntu if unknown, or maybe none? 
        // Returning None might be safer, but let's default to ubuntu for now as it's common.
        None
    }
}

#[async_trait]
impl SourceManager for AptManager {
    fn name(&self) -> &'static str {
        "apt"
    }

    fn requires_sudo(&self) -> bool {
        true
    }

    fn list_candidates(&self) -> Vec<Mirror> {
        let key = format!("apt-{}", self.distro);
        config::get_candidates(&key)
    }

    fn config_path(&self) -> PathBuf {
        if let Some(ref path) = self.custom_path {
            return path.clone();
        }
        PathBuf::from("/etc/apt/sources.list")
    }

    async fn current_url(&self) -> Result<Option<String>> {
        let path = self.config_path();
        if !fs::try_exists(&path).await.unwrap_or(false) {
            return Ok(None);
        }

        let content = fs::read_to_string(&path).await?;
        
        // Find the first active 'deb' line
        // Regex: ^deb\s+(?:\[.*?\]\s+)?(\S+)\s+
        let re = Regex::new(r"(?m)^deb\s+(?:\[.*?\]\s+)?(?P<url>https?://\S+)\s+")?;
        
        if let Some(caps) = re.captures(&content) {
            Ok(Some(caps["url"].to_string()))
        } else {
            Ok(None)
        }
    }

    async fn set_source(&self, mirror: &Mirror) -> Result<()> {
        let path = self.config_path();
        if !fs::try_exists(&path).await.unwrap_or(false) {
             return Err(MirrorError::Custom(format!("Config file not found: {:?}", path)));
        }

        let content = fs::read_to_string(&path).await?;
        utils::backup_file(&path).await?;

        // Strategy: Replace the base URL of the main repo.
        // We need to know what the CURRENT URL is to replace it.
        // But the user might have mixed sources. 
        // Safe bet: Replace lines that look like the distro's main repo.
        
        let target_url = if mirror.url.ends_with('/') {
            mirror.url.clone()
        } else {
            format!("{}/", mirror.url)
        };

        // Determine what to replace.
        // If we found a current URL, replace IT.
        let current = self.current_url().await?;
        
        let new_content = if let Some(cur_url) = current {
            // Replace all occurrences of current_url with mirror.url
            // Note: Use simple string replacement to avoid regex escaping issues, 
            // but be careful about partial matches.
            content.replace(&cur_url, &target_url)
        } else {
            // If we couldn't detect current URL, maybe we shouldn't touch it?
            // Or try to replace known default domains?
            let default_domains = if self.distro == "ubuntu" {
                vec!["archive.ubuntu.com/ubuntu/", "security.ubuntu.com/ubuntu/"]
            } else {
                vec!["deb.debian.org/debian/", "security.debian.org/debian/"]
            };
            
            let mut modified = content.clone();
            for domain in default_domains {
                // Try to replace HTTP and HTTPS variants
                modified = modified.replace(&format!("http://{}", domain), &target_url);
                modified = modified.replace(&format!("https://{}", domain), &target_url);
            }
            modified
        };

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
    async fn test_apt_flow() -> Result<()> {
        let dir = tempdir()?;
        let config_path = dir.path().join("sources.list");
        
        // Prepare a dummy sources.list
        let initial_content = r#"
# Main repo
deb http://archive.ubuntu.com/ubuntu/ jammy main restricted
deb http://archive.ubuntu.com/ubuntu/ jammy-updates main restricted
# Security
deb http://security.ubuntu.com/ubuntu/ jammy-security main restricted
        "#;
        fs::write(&config_path, initial_content).await?;

        let manager = AptManager::with_distro_and_path("ubuntu".to_string(), config_path.clone());

        // 1. Initial detection
        // Note: current_url returns the FIRST match.
        // In our sample, it matches "http://archive.ubuntu.com/ubuntu/"
        assert_eq!(manager.current_url().await?, Some("http://archive.ubuntu.com/ubuntu/".to_string()));

        // 2. Set source
        let mirror = Mirror {
            name: "TestApt".to_string(),
            url: "http://mirrors.test.com/ubuntu/".to_string(),
        };
        manager.set_source(&mirror).await?;

        // 3. Check file content
        let new_content = fs::read_to_string(&config_path).await?;
        assert!(new_content.contains("deb http://mirrors.test.com/ubuntu/ jammy main"));
        // Check if security line also got replaced (depends on logic)
        // Our logic: "Replace all occurrences of current_url with mirror.url"
        // Since current_url was detected as "http://archive.ubuntu.com/ubuntu/", 
        // and security url is "http://security.ubuntu.com/ubuntu/", it might NOT be replaced unless
        // logic falls back to default domains or handles multiple.
        
        // Current logic:
        // let current = self.current_url().await?; // Gets FIRST match
        // if let Some(cur_url) = current { content.replace(&cur_url, &target_url) }
        
        // So it only replaces "archive.ubuntu.com" lines. "security.ubuntu.com" remains.
        // This is actually "safe" behavior (don't mess with security unless intended), 
        // but PRD requirement "apt" usually implies replacing main source.
        
        assert!(new_content.contains("deb http://security.ubuntu.com/ubuntu/ jammy-security"));

        // 4. Restore
        manager.restore().await?;
        let restored_content = fs::read_to_string(&config_path).await?;
        assert_eq!(restored_content, initial_content);

        Ok(())
    }
}
