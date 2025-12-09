use crate::config;
use crate::error::Result;
use crate::traits::SourceManager;
use crate::types::Mirror;
use async_trait::async_trait;
use std::path::PathBuf;

pub struct BrewManager;

impl BrewManager {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl SourceManager for BrewManager {
    fn name(&self) -> &'static str {
        "brew"
    }

    fn requires_sudo(&self) -> bool {
        false
    }

    fn list_candidates(&self) -> Vec<Mirror> {
        config::get_candidates("brew")
    }

    fn config_path(&self) -> PathBuf {
        // Since we are primarily checking environment variables which are transient
        // or stored in shell profiles, we return a symbolic path.
        PathBuf::from("env:HOMEBREW_API_DOMAIN")
    }

    async fn current_url(&self) -> Result<Option<String>> {
        // Check HOMEBREW_API_DOMAIN environment variable
        // This only checks the CURRENT process/shell env, which might not reflect
        // the user's permanent config in .zshrc unless we source it (complex).
        // However, for `cmirror status`, checking env is the correct "current status".
        match std::env::var("HOMEBREW_API_DOMAIN") {
            Ok(val) if !val.is_empty() => Ok(Some(val)),
            _ => Ok(None), // Default is usually https://formulae.brew.sh/api but implicit
        }
    }

    async fn set_source(&self, mirror: &Mirror) -> Result<()> {
        // Since we cannot reliably edit user's shell profile (.zshrc, .bashrc, .config/fish/...)
        // without risk, and `export` only affects current session,
        // we will display the commands the user needs to run.
        //
        // Ideally, `cmirror` would append to the shell profile, but detecting the shell and file is hard.
        // For MVP, we print instructions.

        println!("To apply this mirror, please run the following commands in your terminal:");
        println!();
        println!("    export HOMEBREW_API_DOMAIN \"{}\"", mirror.url);

        // Some mirrors also suggest BOTTLE_DOMAIN, but our JSON currently only tracks one URL.
        // If the URL matches known providers (Tuna/USTC), we can infer the bottle domain.
        if mirror.url.contains("tuna") {
            println!("    export HOMEBREW_BOTTLE_DOMAIN=\"https://mirrors.tuna.tsinghua.edu.cn/homebrew-bottles\"");
        } else if mirror.url.contains("ustc") {
            println!("    export HOMEBREW_BOTTLE_DOMAIN=\"https://mirrors.ustc.edu.cn/homebrew-bottles\"");
        }

        println!();
        println!("To make it permanent, add the above lines to your ~/.zshrc or ~/.bash_profile.");

        // We return Ok because we "handled" the request, even if we didn't write a file.
        // This prevents the main loop from crashing or showing error.
        Ok(())
    }

    async fn restore(&self) -> Result<()> {
        println!("To restore Brew configuration, please unset the environment variables:");
        println!();
        println!("    unset HOMEBREW_API_DOMAIN");
        println!("    unset HOMEBREW_BOTTLE_DOMAIN");
        println!();
        println!("If you added these to your shell profile (~/.zshrc, ~/.bash_profile, etc.), please remove them manually.");
        Ok(())
    }
}
