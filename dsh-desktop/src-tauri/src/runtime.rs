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
    _settings: &AppSettings,
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
    // vendor-dsh 以 npm --prefix 安装:dsh 包位于 node_modules/@deepseek-ai/dsh
    let dsh = resource_dir
        .join("dsh")
        .join("node_modules")
        .join("@deepseek-ai")
        .join("dsh")
        .join("lib")
        .join("bin.js");
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
    let dsh = resolve_dsh_bin(&dsh)?;
    Ok(Runtime {
        node,
        dsh_bin: dsh,
        source: "path",
    })
}

/// dsh 可执行文件 → 真实 lib/bin.js(spawn 时用 `node <bin.js> web`)。
/// - unix: npm 全局 bin 是带 shebang 的 JS(homebrew 场景为符号链接),canonicalize 解析;
/// - windows: npm 生成 dsh.cmd / dsh / dsh.ps1 shim(均非 JS),node 无法直接执行,
///   需解析到同前缀 node_modules/@deepseek-ai/dsh/lib/bin.js。
fn resolve_dsh_bin(p: &Path) -> Result<PathBuf, String> {
    #[cfg(windows)]
    {
        if p.extension().map(|e| e == "js").unwrap_or(false) {
            return Ok(std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()));
        }
        if let Some(dir) = p.parent() {
            let bin = dir
                .join("node_modules")
                .join("@deepseek-ai")
                .join("dsh")
                .join("lib")
                .join("bin.js");
            if bin.is_file() {
                return Ok(bin);
            }
            return Err(format!(
                "找到 dsh shim({}) 但未能解析到 lib/bin.js(期望 {})",
                p.display(),
                bin.display()
            ));
        }
        Err(format!(
            "找到 dsh shim({}) 但无法解析父目录以定位 lib/bin.js",
            p.display()
        ))
    }
    #[cfg(not(windows))]
    {
        Ok(std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()))
    }
}

fn platform_dir() -> &'static str {
    if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            "darwin-arm64"
        } else {
            "darwin-x64"
        }
    } else if cfg!(target_os = "windows") {
        if cfg!(target_arch = "aarch64") {
            "win32-arm64"
        } else {
            "win32-x64"
        }
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
        #[cfg(windows)]
        {
            // Windows 需要 PATHEXT 语义:可执行文件带扩展名(node.exe / dsh.cmd),
            // 无扩展名的裸文件名 is_file() 恒为 false
            for ext in ["", ".exe", ".cmd", ".bat"] {
                let p = dir.join(format!("{name}{ext}"));
                if p.is_file() {
                    return Some(p);
                }
            }
        }
        #[cfg(not(windows))]
        {
            let p = dir.join(name);
            if p.is_file() {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(md) = p.metadata() {
                    if md.permissions().mode() & 0o111 != 0 {
                        return Some(p);
                    }
                }
            }
        }
    }
    None
}
