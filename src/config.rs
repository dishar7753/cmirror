use crate::types::Mirror;
use directories::ProjectDirs;
use std::collections::HashMap;
use std::fs;
use std::sync::OnceLock;

// Include the JSON file at compile time
const MIRRORS_JSON: &str = include_str!("../assets/mirrors.json");

// Global cache for parsed mirrors
static MIRRORS_CACHE: OnceLock<HashMap<String, Vec<Mirror>>> = OnceLock::new();

/// Retrieve the list of mirror candidates for a given tool
/// Strategy:
/// 1. Try to load from User Config (~/.config/cmirror/mirrors.json)
/// 2. Fallback to built-in assets/mirrors.json
pub fn get_candidates(tool_name: &str) -> Vec<Mirror> {
    let mirrors = MIRRORS_CACHE.get_or_init(|| {
        // 1. Try local config
        if let Some(proj_dirs) = ProjectDirs::from("", "", "cmirror") {
            let config_path = proj_dirs.config_dir().join("mirrors.json");
            if config_path.exists() {
                if let Ok(content) = fs::read_to_string(&config_path) {
                    if let Ok(parsed) = serde_json::from_str(&content) {
                        println!("Loaded mirrors from local config: {:?}", config_path);
                        return parsed;
                    }
                }
            }
        }

        // 2. Fallback
        serde_json::from_str(MIRRORS_JSON)
            .expect("Failed to parse assets/mirrors.json. This is a compile-time error.")
    });

    mirrors.get(tool_name).cloned().unwrap_or_default()
}
