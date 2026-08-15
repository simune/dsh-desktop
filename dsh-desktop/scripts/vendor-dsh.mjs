#!/usr/bin/env node
/**
 * vendor-dsh.mjs —— 安装 dsh 依赖树到 src-tauri/resources/dsh。
 * 版本锁定;--omit=dev;输出运行时 manifest。对应 docs/03 §3.1。
 */
import { execSync } from 'node:child_process';
import { mkdir, writeFile, rm } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const DSH_VERSION = process.env.DSH_DESKTOP_DSH_VERSION ?? '0.1.0-rc.6';
const __dirname = path.dirname(fileURLToPath(import.meta.url));
const PLUGIN_DIR = process.env.DSH_DESKTOP_PLUGIN_DIR ??
  path.join(__dirname, '..', '..', 'plugins', 'dsh-usage-stats');
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

  // 捆绑 dsh-usage-stats 插件到安装树:解析时"安装树优先于 profile",离线可用
  const pluginPkg = path.join(PLUGIN_DIR, 'package.json');
  let pluginVendored = false;
  if (fs.existsSync(pluginPkg)) {
    console.log(`[vendor-dsh] 捆绑插件 dsh-usage-stats <- ${PLUGIN_DIR}`);
    const tmp = path.join(DSH_DIR, '.vendor-tmp');
    await mkdir(tmp, { recursive: true });
    const packed = execSync(`npm pack "${PLUGIN_DIR}" --pack-destination "${tmp}" --silent`, { encoding: 'utf8' }).trim();
    const tgz = path.join(tmp, packed);
    execSync(`npm install --prefix "${DSH_DIR}" "${tgz}" --omit=dev --no-audit --no-fund`, { stdio: 'inherit' });
    await rm(tmp, { recursive: true, force: true });
    pluginVendored = fs.existsSync(path.join(DSH_DIR, 'node_modules', 'dsh-usage-stats', 'package.json'));
  } else {
    console.warn('[vendor-dsh] 未找到插件目录,跳过 dsh-usage-stats 捆绑');
  }

  const manifest = {
    dshVersion: DSH_VERSION,
    bin: 'lib/bin.js',
    plugin: pluginVendored ? 'dsh-usage-stats' : null,
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
