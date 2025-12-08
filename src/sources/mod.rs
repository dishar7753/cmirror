pub mod pip;
pub mod docker;
pub mod npm;
pub mod go;
pub mod cargo;
pub mod brew;
pub mod apt;

use crate::traits::SourceManager;
use crate::error::{Result, MirrorError};

pub const SUPPORTED_TOOLS: &[&str] = &["pip", "npm", "docker", "go", "cargo", "brew", "apt"];

pub fn get_manager(name: &str) -> Result<Box<dyn SourceManager>> {
    match name.to_lowercase().as_str() {
        "pip" => Ok(Box::new(pip::PipManager::new())),
        "docker" => Ok(Box::new(docker::DockerManager::new())),
        "npm" => Ok(Box::new(npm::NpmManager::new())),
        "go" => Ok(Box::new(go::GoManager::new())),
        "cargo" => Ok(Box::new(cargo::CargoManager::new())),
        "brew" => Ok(Box::new(brew::BrewManager::new())),
        "apt" => Ok(Box::new(apt::AptManager::new())),
        _ => Err(MirrorError::UnknownTool(format!(
            "Unsupported tool: '{}'. Available: {}", 
            name, 
            SUPPORTED_TOOLS.join(", ")
        ))),
    }
}