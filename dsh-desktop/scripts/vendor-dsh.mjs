#!/usr/bin/env node
/**
 * vendor-dsh.mjs —— 安装 dsh 依赖树到 src-tauri/resources/dsh。
 * 版本锁定;--omit=dev;输出运行时 manifest。对应 docs/03 §3.1。
 */
import { execSync } from 'node:child_process';
import { mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const DSH_VERSION = process.env.DSH_DESKTOP_DSH_VERSION ?? '0.1.0-rc.6';
const __dirname = path.dirname(fileURLToPath(import.meta.url));
const DSH_DIR = path.join(__dirname, '..', 'src-tauri', 'resources', 'dsh');

async function main() {
  await mkdir(DSH_DIR, { recursive: true });
  console.log(`[vendor-dsh] npm install @deepseek-ai/dsh@${DSH_VERSION} --omit=dev (may take a while)`);
  execSync(
    `npm install --prefix "${DSH_DIR}" @deepseek-ai/dsh@${DSH_VERSION} --omit=dev --no-audit --no-fund --no-package-lock=false`,
    { stdio: 'inherit', cwd: process.cwd() },
  );

  // 校验安装树
  const binJs = path.join(DSH_DIR, 'node_modules', '@deepseek-ai', 'dsh', 'lib', 'bin.js');
  const fs = await import('node:fs');
  if (!fs.existsSync(binJs)) {
    throw new Error(`安装树不完整: 缺少 ${binJs}`);
  }

  const manifest = {
    dshVersion: DSH_VERSION,
    bin: 'lib/bin.js',
    installedAt: new Date().toISOString(),
  };
  await writeFile(path.join(DSH_DIR, 'dsh-manifest.json'), JSON.stringify(manifest, null, 2) + '\n');

  // ExFAT/APFS 卷会生成 ._ AppleDouble 侧车,必须清理(否则被打进安装包,体积翻倍);
  // Windows 无此问题,且无 find 命令,直接跳过
  if (process.platform !== 'win32') {
    execSync(`find "${DSH_DIR}" -name '._*' -delete`, { stdio: 'inherit' });
  }
  console.log(`[vendor-dsh] 完成: ${DSH_DIR} (AppleDouble 侧车已清理)`);
}

main().catch((e) => {
  console.error('[vendor-dsh] 失败:', e.message);
  process.exit(1);
});
