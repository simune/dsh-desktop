#!/usr/bin/env node
/**
 * build-release.mjs —— 一键重新编译并打包 DSH Desktop（macOS / Windows）。
 *
 * 用法:
 *   node scripts/build-release.mjs
 *   npm run build:release
 *
 * 选项:
 *   --skip-install   跳过 npm install
 *   --skip-vendor    跳过 vendor（resources 未变时可加速）
 *   --no-bundle      仅 release 编译，不生成安装包
 *   --msi            Windows: 打 MSI（默认 NSIS）
 *   --local-target   使用 src-tauri/target（不写入系统缓存目录）
 *   -h, --help       显示帮助
 *
 * 环境变量:
 *   CARGO_TARGET_DIR              覆盖 Rust 构建目录
 *   DSH_DESKTOP_USE_LOCAL_TARGET=1  等同 --local-target
 */
import { spawn } from 'node:child_process';
import { existsSync, readdirSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, '..');
const TAURI_DIR = path.join(ROOT, 'src-tauri');

const isWin = process.platform === 'win32';
const isMac = process.platform === 'darwin';

function usage() {
  console.log(`DSH Desktop 一键构建打包（${process.platform}/${process.arch}）

用法:
  node scripts/build-release.mjs [选项]
  npm run build:release [-- 选项]

流程:
  1. npm install（可跳过）
  2. 清理 AppleDouble 侧车
  3. vendor node + dsh（可跳过）
  4. 前端构建（tsc + vite）
  5. tauri build → 安装包

选项:
  --skip-install   跳过 npm install
  --skip-vendor    跳过 vendor
  --no-bundle      仅编译 release，不打安装包
  --msi            Windows: 生成 MSI（默认 NSIS）
  --local-target   构建产物落在 src-tauri/target
  -h, --help       显示本帮助

产物目录:
  macOS:   <CARGO_TARGET_DIR>/release/bundle/dmg/*.dmg
  Windows: <CARGO_TARGET_DIR>/release/bundle/nsis/*.exe
`);
}

function parseArgs(argv) {
  const opts = {
    skipInstall: false,
    skipVendor: false,
    noBundle: false,
    msi: false,
    localTarget: process.env.DSH_DESKTOP_USE_LOCAL_TARGET === '1',
    help: false,
  };
  for (const arg of argv) {
    switch (arg) {
      case '-h':
      case '--help':
        opts.help = true;
        break;
      case '--skip-install':
        opts.skipInstall = true;
        break;
      case '--skip-vendor':
        opts.skipVendor = true;
        break;
      case '--no-bundle':
        opts.noBundle = true;
        break;
      case '--msi':
        opts.msi = true;
        break;
      case '--local-target':
        opts.localTarget = true;
        break;
      default:
        console.error(`未知参数: ${arg}`);
        usage();
        process.exit(2);
    }
  }
  return opts;
}

function defaultCargoTargetDir() {
  if (isMac) {
    return path.join(os.homedir(), 'Library', 'Caches', 'dsh-desktop', 'target');
  }
  if (isWin) {
    const base = process.env.LOCALAPPDATA || path.join(os.homedir(), 'AppData', 'Local');
    return path.join(base, 'dsh-desktop', 'target');
  }
  return path.join(os.homedir(), '.cache', 'dsh-desktop', 'target');
}

function resolveCargoTargetDir(opts) {
  if (process.env.CARGO_TARGET_DIR) {
    return process.env.CARGO_TARGET_DIR;
  }
  if (opts.localTarget) {
    return path.join(TAURI_DIR, 'target');
  }
  return defaultCargoTargetDir();
}

function npmCmd() {
  return isWin ? 'npm.cmd' : 'npm';
}

function npxCmd() {
  return isWin ? 'npx.cmd' : 'npx';
}

function run(cmd, args, extraEnv = {}) {
  return new Promise((resolve, reject) => {
    const label = [cmd, ...args].join(' ');
    console.log(`\n[build-release] >>> ${label}`);
    const child = spawn(cmd, args, {
      cwd: ROOT,
      stdio: 'inherit',
      env: { ...process.env, ...extraEnv },
      shell: isWin,
    });
    child.on('error', reject);
    child.on('exit', (code) => {
      if (code === 0) resolve();
      else reject(new Error(`命令失败 (exit ${code}): ${label}`));
    });
  });
}

function collectArtifacts(targetDir) {
  const bundleDir = path.join(targetDir, 'release', 'bundle');
  if (!existsSync(bundleDir)) return [];

  const out = [];
  const walk = (dir) => {
    for (const name of readdirSync(dir, { withFileTypes: true })) {
      const p = path.join(dir, name.name);
      if (name.isDirectory()) walk(p);
      else if (/\.(dmg|exe|msi|app)$/i.test(name.name)) out.push(p);
    }
  };
  walk(bundleDir);
  return out.sort();
}

async function main() {
  const opts = parseArgs(process.argv.slice(2));
  if (opts.help) {
    usage();
    return;
  }

  if (!isMac && !isWin) {
    console.error('[build-release] 当前仅支持 macOS 与 Windows。');
    process.exit(1);
  }

  const cargoTargetDir = resolveCargoTargetDir(opts);
  const env = { CARGO_TARGET_DIR: cargoTargetDir };
  const t0 = Date.now();

  console.log('[build-release] DSH Desktop 一键构建');
  console.log(`[build-release] 平台: ${process.platform}/${process.arch}`);
  console.log(`[build-release] CARGO_TARGET_DIR: ${cargoTargetDir}`);

  if (!opts.skipInstall) {
    await run(npmCmd(), ['install'], env);
  }

  await run(npmCmd(), ['run', 'clean:apple-double'], env);

  if (!opts.skipVendor) {
    await run(npmCmd(), ['run', 'vendor'], env);
  }

  await run(npmCmd(), ['run', 'build'], env);

  const tauriArgs = ['tauri', 'build'];
  if (opts.noBundle) {
    tauriArgs.push('--no-bundle');
  } else if (opts.msi) {
    if (!isWin) {
      console.error('[build-release] --msi 仅适用于 Windows。');
      process.exit(1);
    }
    tauriArgs.push('--bundles', 'msi');
  }

  await run(npxCmd(), tauriArgs, env);

  const elapsed = ((Date.now() - t0) / 1000).toFixed(1);
  console.log(`\n[build-release] 完成，耗时 ${elapsed}s`);

  const artifacts = collectArtifacts(cargoTargetDir);
  if (artifacts.length > 0) {
    console.log('[build-release] 安装包 / 产物:');
    for (const p of artifacts) console.log(`  - ${p}`);
  } else {
    const bin = isWin ? 'dsh-desktop.exe' : 'dsh-desktop';
    console.log(`[build-release] 可执行文件: ${path.join(cargoTargetDir, 'release', bin)}`);
  }
}

main().catch((err) => {
  console.error(`\n[build-release] 失败: ${err.message}`);
  process.exit(1);
});
