use async_trait::async_trait;
use crate::error::Result;
use crate::types::Mirror;
use std::path::PathBuf;

/// SourceManager: 所有镜像源管理模块必须实现的接口
#[async_trait]
pub trait SourceManager: Sync + Send {
    /// 工具名称 (如 "pip", "docker")
    fn name(&self) -> &'static str;

    /// 是否需要 Root 权限 (如 apt, docker 需要 sudo)
    fn requires_sudo(&self) -> bool;

    /// 获取内置的推荐源列表
    fn list_candidates(&self) -> Vec<Mirror>;

    /// 获取当前正在使用的源 URL
    /// 返回 Option: 如果未配置或无法解析，则返回 None (视为默认)
    async fn current_url(&self) -> Result<Option<String>>;

    /// 应用新的镜像源
    /// 实现中必须包含:
    /// 1. 备份原配置文件
    /// 2. 写入新配置
    async fn set_source(&self, mirror: &Mirror) -> Result<()>;

    /// 获取配置文件的路径 (用于日志显示或备份)
    fn config_path(&self) -> PathBuf;

    /// 恢复到上一次的配置 (或默认配置)
    async fn restore(&self) -> Result<()>;
}