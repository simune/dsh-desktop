//! 应用设置持久化(JSON):DSH_HOME、端口策略、cwd、日志行数。
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    /// 空 = 跟随环境变量 DSH_HOME / 默认 ~/.dsh
    pub dsh_home: Option<String>,
    pub port_policy: PortPolicy,
    /// 子进程工作目录,空 = 用户主目录
    pub cwd: Option<String>,
    pub autostart: bool,
    pub log_lines: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum PortPolicy {
    Auto,
    Fixed { port: u16 },
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            dsh_home: None,
            port_policy: PortPolicy::Auto,
            cwd: None,
            autostart: false,
            log_lines: 2000,
        }
    }
}

impl AppSettings {
    pub fn load(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let s = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(path, s).map_err(|e| e.to_string())
    }
}
