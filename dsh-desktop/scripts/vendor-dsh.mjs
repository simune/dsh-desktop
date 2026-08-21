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
    // 插件源码需先构建(npm pack 默认只打包 files 声明,如 lib/**;缺构建产物会打进空包,
    // 导致 dsh 启动 ERR_MODULE_NOT_FOUND)。构建是幂等的,依赖缺失时先 install。
    const pluginBin = path.join(PLUGIN_DIR, 'lib', 'index.js');
    if (!fs.existsSync(pluginBin)) {
      console.log('[vendor-dsh]   插件缺 lib/ 构建产物,先执行构建…');
      const needsInstall = !fs.existsSync(path.join(PLUGIN_DIR, 'node_modules', '.bin'));
      if (needsInstall) {
        console.log('[vendor-dsh]   插件依赖缺失,先 npm install…');
        execSync(`npm install --prefix "${PLUGIN_DIR}" --no-audit --no-fund`, { stdio: 'inherit', cwd: process.cwd() });
      }
      execSync(`npm run build --prefix "${PLUGIN_DIR}"`, { stdio: 'inherit', cwd: process.cwd() });
      if (!fs.existsSync(pluginBin)) {
        throw new Error(`插件构建后仍缺 ${pluginBin},请检查 ${PLUGIN_DIR} 的 build 脚本`);
      }
      console.log(`[vendor-dsh]   插件构建完成:${pluginBin}`);
    }
    const tmp = path.join(DSH_DIR, '.vendor-tmp');
    await mkdir(tmp, { recursive: true });
    const packed = execSync(`npm pack "${PLUGIN_DIR}" --pack-destination "${tmp}" --silent`, { encoding: 'utf8' }).trim();
    const tgz = path.join(tmp, packed);
    execSync(`npm install --prefix "${DSH_DIR}" "${tgz}" --omit=dev --no-audit --no-fund`, { stdio: 'inherit' });
    await rm(tmp, { recursive: true, force: true });
    pluginVendored = fs.existsSync(path.join(DSH_DIR, 'node_modules', 'dsh-usage-stats', 'package.json'));
    // 校验打包产物确实含 lib/index.js(防止 files 声明与实际不符的空包)
    const vendoredLib = path.join(DSH_DIR, 'node_modules', 'dsh-usage-stats', 'lib', 'index.js');
    if (!fs.existsSync(vendoredLib)) {
      throw new Error(`插件打包产物缺 lib/index.js:${vendoredLib};请检查插件 package.json 的 files 声明`);
    }
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

  // 保守裁剪依赖树(减少打包文件数 → NSIS 解包提速);不确定的一律保留
  console.log('[vendor-dsh] 保守裁剪依赖树(prune-dsh-tree.mjs)…');
  execSync(`node "${path.join(__dirname, 'prune-dsh-tree.mjs')}"`, { stdio: 'inherit', cwd: process.cwd() });

  // 裁剪后冒烟验证:dsh 仍可启动并打印 URL 行(server 长驻,匹配到 URL 行即通过并杀掉)
  const nodeBin = path.join(DSH_DIR, '..', 'node', process.platform === 'win32' ? 'win32-x64' : process.platform === 'darwin' ? (process.arch === 'arm64' ? 'darwin-arm64' : 'darwin-x64') : 'linux-x64', process.platform === 'win32' ? 'node.exe' : 'node');
  if (fs.existsSync(nodeBin)) {
    console.log('[vendor-dsh] 冒烟验证:启动 dsh web(--no-open)…');
    const { spawn } = await import('node:child_process');
    const smoke = spawn(nodeBin, [binJs, 'web', '--port', '0', '--no-open'], {
      windowsHide: true,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    let urlLine = null;
    const smokeTimeout = setTimeout(() => {
      console.error('[vendor-dsh] 冒烟失败:60s 内未解析到 URL 行');
      smoke.kill();
      process.exit(1);
    }, 60000);
    smoke.stdout.on('data', (d) => {
      const text = d.toString();
      process.stdout.write(text);
      if (!urlLine && /dsh web: http:\/\/127\.0\.0\.1:\d+/.test(text)) {
        urlLine = (text.match(/dsh web: http:\/\/127\.0\.0\.1:\d+/) || [''])[0];
      }
    });
    smoke.stderr.on('data', (d) => process.stderr.write(d));
    smoke.on('exit', (code) => {
      if (!urlLine) {
        console.error(`[vendor-dsh] 冒烟失败:子进程退出 code=${code},未解析到 URL 行`);
        process.exit(1);
      }
    });
    await new Promise((resolve) => {
      const check = setInterval(() => {
        if (urlLine) {
          clearInterval(check);
          clearTimeout(smokeTimeout);
          console.log(`[vendor-dsh] 冒烟通过:${urlLine}`);
          smoke.kill();
          resolve();
        }
      }, 200);
    });
  } else {
    console.warn('[vendor-dsh] 未找到 bundled node,跳过冒烟验证');
  }

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
