use std::path::Path;
use serde::Deserialize;

/// JSON 配置文件结构
#[derive(Debug, Deserialize)]
pub struct Config {
    pub username: String,
    pub password: String,
}

/// 从 JSON 文件读取配置
pub fn read_config(path: &Path) -> Result<Config, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(path)?;
    let config: Config = serde_json::from_str(&content)?;
    Ok(config)
}
