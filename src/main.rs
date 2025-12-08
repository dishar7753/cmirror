mod config;
mod error;
mod sources;
mod traits;
mod types;
mod utils;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use sources::get_manager;
use types::Mirror;

#[derive(Parser)]
#[command(name = "cmirror")]
#[command(about = "A high-performance mirror manager for China", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show current configuration (e.g., cmirror status [pip])
    Status {
        /// The tool name (pip, docker, etc.). If omitted, shows all.
        name: Option<String>,
    },
    /// Benchmark mirrors (e.g., cmirror test pip)
    Test {
        /// The tool name
        name: String,
    },
    /// Apply a new mirror (e.g., cmirror use pip --fastest)
    Use {
        /// The tool name
        name: String,

        /// Mirror alias (e.g., Aliyun)
        #[arg(required_unless_present = "fastest")]
        source: Option<String>,

        /// Auto-select the fastest mirror
        #[arg(long, short)]
        fastest: bool,
    },
    /// Restore the configuration to the previous backup or default
    Restore {
        /// The tool name
        name: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Status { name } => handle_status(name).await?,
        Commands::Test { name } => handle_test(&name).await?,
        Commands::Use {
            name,
            source,
            fastest,
        } => handle_use(&name, source, fastest).await?,
        Commands::Restore { name } => handle_restore(&name).await?,
    }

    Ok(())
}

// --- Handlers ---

async fn handle_status(name: Option<String>) -> Result<()> {
    let tools = match name {
        Some(n) => vec![n],
        None => sources::SUPPORTED_TOOLS
            .iter()
            .map(|&s| s.to_string())
            .collect(),
    };

    println!("{}", "-".repeat(70));
    println!("{:<10} {:<40} Status", "Tool", "Current Source URL");
    println!("{}", "-".repeat(70));

    for tool_name in tools {
        let manager = match get_manager(&tool_name) {
            Ok(m) => m,
            Err(_) => continue,
        };

        // Handle potential errors gracefully instead of crashing the whole status command
        let current_url_res = manager.current_url().await;
        
        let current_url = current_url_res.unwrap_or_default();

        let candidates = manager.list_candidates();

        let (url_display, status_display) = match current_url {
            Some(url) => {
                // Check if it matches any known candidate
                let known_name = candidates
                    .iter()
                    .find(|m| m.url.trim_end_matches('/') == url.trim_end_matches('/'))
                    .map(|m| m.name.clone())
                    .unwrap_or_else(|| "Custom".to_string());

                (url, format!("[{}]", known_name))
            }
            None => ("Default".to_string(), "[Official/Default]".to_string()),
        };

        // Truncate URL if too long
        let mut url_short = url_display.clone();
        if url_short.len() > 38 {
            url_short = format!("{}...", &url_short[..35]);
        }

        println!(
            "{:<10} {:<40} {}",
            manager.name(),
            url_short,
            status_display
        );
    }
    println!("{}", "-".repeat(70));

    Ok(())
}
async fn handle_test(name: &str) -> Result<()> {
    let manager = get_manager(name)?;
    let mut candidates = manager.list_candidates();

    // 1. Determine the "Current" URL
    //    - If config exists, use it.
    //    - If not, try to default to "Official" from candidates.
    let mut current_url_opt = manager.current_url().await.ok().flatten();

    if current_url_opt.is_none() {
         // Fallback to Official if available in candidates
         if let Some(official) = candidates.iter().find(|m| m.name.eq_ignore_ascii_case("Official")) {
             current_url_opt = Some(official.url.clone());
         }
    }

    // 2. Add "Current" to candidates if it's a custom one (not in list)
    if let Some(ref current_url) = current_url_opt {
        // Check if this URL is already in candidates (normalized check)
        let is_known = candidates.iter().any(|m| 
            m.url == *current_url || 
            m.url.trim_end_matches('/') == current_url.trim_end_matches('/')
        );
        
        if !is_known {
            candidates.push(Mirror::new("Current", current_url));
        }
    }

    let results = utils::benchmark_mirrors(candidates).await;
    
    println!(); // Newline after progress bar
    println!(); // Additional newline for visual separation

    // Print Table
    println!("{:<4} {:<10} {:<12} URL", "RANK", "LATENCY", "NAME");
    println!("{}", "-".repeat(60));

    for (i, res) in results.iter().enumerate() {
        let latency_str = if res.latency_ms == u64::MAX {
            "Timeout".to_string()
        } else {
            format!("{}ms", res.latency_ms)
        };

        println!(
            "{:<4} {:<10} {:<12} {}",
            i + 1,
            latency_str,
            res.mirror.name,
            res.mirror.url
        );
    }

    // Recommendation
    if let Some(best) = results.first() {
        if best.latency_ms < u64::MAX {
            println!("{}", "-".repeat(60));
            
            let mut speedup_msg = None;
            
            if let Some(ref current_url) = current_url_opt {
                 // Find the result corresponding to current_url
                 let current_res = results.iter().find(|r| 
                    r.mirror.url == *current_url || 
                    r.mirror.url.trim_end_matches('/') == current_url.trim_end_matches('/')
                 );

                 if let Some(cur) = current_res {
                     if cur.latency_ms < u64::MAX {
                         if cur.latency_ms > best.latency_ms {
                             let speedup = cur.latency_ms as f64 / best.latency_ms as f64;
                             speedup_msg = Some(format!("Recommendation: '{}' is {:.1}x faster than your current source.", best.mirror.name, speedup));
                         } else if cur.latency_ms == best.latency_ms {
                             speedup_msg = Some(format!("Recommendation: Your current source '{}' is already the fastest.", cur.mirror.name));
                         }
                     } else {
                         speedup_msg = Some(format!("Recommendation: '{}' is significantly faster than your current source (Timeout).", best.mirror.name));
                     }
                 }
            }
            
            if let Some(msg) = speedup_msg {
                println!("{}", msg);
            } else {
                 println!("Recommendation: '{}' is the fastest.", best.mirror.name);
            }
            
            println!(
                "Run 'cmirror use {} {}' to apply.",
                name, best.mirror.name
            );
        }
    }

    Ok(())
}

async fn handle_use(name: &str, source_name: Option<String>, fastest: bool) -> Result<()> {
    let manager = get_manager(name)?;

    // 检查权限
    if manager.requires_sudo() {
        eprintln!(
            "Note: Modifying {} config usually requires sudo/root permissions.",
            name
        );
    }

    // 这一大段代码是为了计算出 target_mirror
    // 注意：整个 if-else 表达式最后需要一个分号
    let target_mirror = if fastest {
        println!("Finding fastest mirror...");
        let results = utils::benchmark_mirrors(manager.list_candidates()).await;

        // 过滤掉超时的 (u64::MAX)
        let valid_results: Vec<_> = results
            .into_iter()
            .filter(|r| r.latency_ms < u64::MAX)
            .collect();

        if valid_results.is_empty() {
            bail!("All mirrors timed out. Please check your network connection.");
        }

        let best = &valid_results[0];
        println!(
            "Fastest mirror is {} ({}ms)",
            best.mirror.name, best.latency_ms
        );
        best.mirror.clone() // 返回给 target_mirror
    } else {
        // 按名称查找
        // unwrap 是安全的，因为 clap 配置中 required_unless_present = "fastest" 保证了 source_name 存在
        let target_name = source_name.unwrap();
        let candidates = manager.list_candidates();

        match candidates
            .into_iter()
            .find(|m| m.name.eq_ignore_ascii_case(&target_name))
        {
            Some(m) => m, // 返回给 target_mirror
            None => bail!(
                "Mirror '{}' not found. Use 'test' to see available list.",
                target_name
            ),
        }
    }; // <--- 这里的由 if/else 构成的 let 语句必须以分号结束

    println!("Backing up and applying {}...", target_mirror.name);
    manager.set_source(&target_mirror).await?;
    println!("Success! {} is now using {}.", name, target_mirror.name);

    Ok(())
}

async fn handle_restore(name: &str) -> Result<()> {
    let manager = get_manager(name)?;

    if manager.requires_sudo() {
        eprintln!(
            "Note: Restoring {} config usually requires sudo/root permissions.",
            name
        );
    }

    println!("Restoring {} configuration...", name);
    manager.restore().await?;
    println!("Success! {} configuration restored.", name);

    Ok(())
}
