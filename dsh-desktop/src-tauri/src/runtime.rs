//! 运行时探测链:bundled(resources)→ 系统 PATH。
//! 对应 docs/03 §5;bundled 由 M2 的 vendor 脚本生成。
use crate::settings::AppSettings;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Runtime {
    pub node: PathBuf,
    pub dsh_bin: PathBuf,
    pub source: &'static str,
}

pub fn resolve_runtime(
    settings: &AppSettings,
    resource_dir: &Path,
) -> Result<Runtime, String> {
    // 0. 环境变量显式覆盖(调试/测试/用户配置,对应 docs/03 §5 探测链)
    if let Ok(node) = std::env::var("DSH_DESKTOP_NODE") {
        if let Ok(dsh) = std::env::var("DSH_DESKTOP_DSH") {
            let node = PathBuf::from(node);
            let dsh = PathBuf::from(dsh);
            if node.is_file() && dsh.is_file() {
                return Ok(Runtime {
                    node,
                    dsh_bin: dsh,
                    source: "config",
                });
            }
        }
    }
    // 1. bundled(优先)
    let node = resource_dir
        .join("node")
        .join(platform_dir())
        .join(node_exe());
    let dsh = resource_dir.join("dsh").join("lib").join("bin.js");
    if node.is_file() && dsh.is_file() {
        return Ok(Runtime {
            node,
            dsh_bin: dsh,
            source: "bundled",
        });
    }
    // 2. 系统 PATH
    let node = find_executable("node").ok_or("未在 PATH 找到 node")?;
    let dsh = find_executable("dsh")
        .ok_or("未在 PATH 找到 dsh;请先 npm i -g @deepseek-ai/dsh 或配置 bundled 运行时")?;
    // dsh 可能是符号链接(homebrew),解析到真实 bin.js
    let dsh = std::fs::canonicalize(&dsh).unwrap_or(dsh);
    Ok(Runtime {
        node,
        dsh_bin: dsh,
        source: "path",
    })
}

fn platform_dir() -> &'static str {
    if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            "darwin-arm64"
        } else {
            "darwin-x64"
        }
    } else if cfg!(target_os = "windows") {
        "win32-x64"
    } else {
        "linux-x64"
    }
}

fn node_exe() -> &'static str {
    if cfg!(target_os = "windows") {
        "node.exe"
    } else {
        "node"
    }
}

fn find_executable(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let p = dir.join(name);
        if p.is_file() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(md) = p.metadata() {
                    if md.permissions().mode() & 0o111 != 0 {
                        return Some(p);
                    }
                }
            }
            #[cfg(not(unix))]
            {
                return Some(p);
            }
        }
    }
    None
}
