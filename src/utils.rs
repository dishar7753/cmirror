use crate::error::{MirrorError, Result};
use crate::types::{BenchmarkResult, Mirror};
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::fs;

// 设置全局请求超时，防止慢源阻塞整个流程太久
const REQUEST_TIMEOUT: u64 = 3;

/// 备份文件 (如果有)
/// 文件名格式: original.ext -> original.ext.bak.TIMESTAMP
pub async fn backup_file(path: &Path) -> Result<()> {
    if fs::try_exists(path).await.unwrap_or(false) {
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        // 这里的命名策略：如果有扩展名，插在扩展名后面还是直接追加？
        // 之前的实现是: path.with_extension(format!("npmrc.bak.{}", timestamp))
        // 这实际上是替换了扩展名。
        // 更好的做法通常是直接在文件名后面追加 .bak.timestamp，保留原扩展名信息
        // 但为了保持和之前代码行为的一致性 (或者优化它)，这里我选择直接追加后缀
        // 例如: config.json -> config.json.bak.123456
        let file_name = path.file_name().unwrap_or_default().to_string_lossy();
        let backup_name = format!("{}.bak.{}", file_name, timestamp);
        let backup_path = path.with_file_name(backup_name);

        fs::copy(path, &backup_path).await?;
        println!("Backup created at: {:?}", backup_path);
    }
    Ok(())
}

/// 恢复到最近的备份
pub async fn restore_latest_backup(path: &Path) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
    let prefix = format!("{}.bak.", file_name);

    if !fs::try_exists(parent).await.unwrap_or(false) {
        return Err(MirrorError::Custom(format!(
            "Directory not found: {:?}",
            parent
        )));
    }

    let mut entries = fs::read_dir(parent).await?;
    let mut backups = Vec::new();

    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(&prefix) {
            backups.push(entry.path());
        }
    }

    if backups.is_empty() {
        return Err(MirrorError::Custom("No backup files found.".to_string()));
    }

    // Sort by path string (effectively sorting by timestamp suffix)
    backups.sort();

    // Get the last one (latest timestamp)
    let latest = backups.last().unwrap();

    println!("Restoring from backup: {:?}", latest);
    fs::copy(latest, path).await?;
    println!("Successfully restored configuration.");

    Ok(())
}

/// 并发测试所有镜像源的延迟
///
/// 逻辑:
/// 1. 构建带有超时设置的 HTTP Client
/// 2. 为每个镜像源生成一个异步任务 (Task)
/// 3. 并行等待所有任务完成 (join_all)
/// 4. 按延迟从小到大排序结果
pub async fn benchmark_mirrors(mirrors: Vec<Mirror>) -> Vec<BenchmarkResult> {
    // 构建 Client, 强制设置超时
    let client = Client::builder()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT))
        .build()
        .unwrap_or_default();

    let pb = ProgressBar::new(mirrors.len() as u64);
    pb.set_style(
        ProgressStyle::with_template("[{bar:40.cyan/blue}] {percent}% {msg}")
            .unwrap()
            .progress_chars("|| "),
    );
    pb.set_message("Testing...");

    // 映射 Mirror -> Future
    let tasks = mirrors.into_iter().map(|m| {
        let client = client.clone();
        let pb = pb.clone();
        async move {
            let res = check_latency(&client, m).await;
            pb.inc(1);
            res
        }
    });

    // 并发执行所有 Future

    let mut results = futures::future::join_all(tasks).await;

    pb.finish_with_message("Testing completed.");

    // 排序: 延迟低的在前, 失败的(MAX)在后

    results.sort_by_key(|r| r.latency_ms);

    results
}

/// 单个源测速逻辑
async fn check_latency(client: &Client, mirror: Mirror) -> BenchmarkResult {
    let start = Instant::now();

    // Clean URL for benchmarking (remove cargo's "sparse+" or "git+" prefixes)

    let url_to_test = mirror
        .url
        .trim_start_matches("sparse+")
        .trim_start_matches("git+");

    // 使用 HEAD 请求而不是 GET，只获取元数据，速度更快且省流量

    // 很多镜像源根路径不一定响应，建议 URL 带有具体路径 (如 /simple)

    let request = client.head(url_to_test).send();

    let latency_ms = match request.await {
        Ok(resp) => {
            if resp.status().is_success() {
                // 计算 TTFB (Time To First Byte)

                start.elapsed().as_millis() as u64
            } else {
                // 虽然连上了，但返回 404/500 等错误，视为不可用

                u64::MAX
            }
        }

        Err(_) => {
            // 连接超时、DNS 解析失败等

            u64::MAX
        }
    };

    BenchmarkResult { mirror, latency_ms }
}
