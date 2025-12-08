use serde::{Deserialize, Serialize};

/// 镜像源定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mirror {
    pub name: String,   // 例如: "Aliyun"
    pub url: String,    // 例如: "https://mirrors.aliyun.com/pypi/simple/"
}

impl Mirror {
    pub fn new(name: &str, url: &str) -> Self {
        Self {
            name: name.to_string(),
            url: url.to_string(),
        }
    }
}

/// 测速结果
#[derive(Debug, Clone)]
pub struct BenchmarkResult {
    pub mirror: Mirror,
    pub latency_ms: u64, // 延迟 (毫秒), 若失败则设为 u64::MAX
}