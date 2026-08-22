#!/usr/bin/env node
/**
 * vendor-dsh.mjs —— 安装 dsh 依赖树到 src-tauri/resources/dsh。
 * 版本锁定;--omit=dev;输出运行时 manifest。对应 docs/03 §3.1。
 *
 * 缓存策略:
 * 1. 若 dsh-manifest + 安装树与插件状态均匹配 → 跳过 npm install / prune / 冒烟
 * 2. --force 或 DSH_DESKTOP_VENDOR_DSH_FORCE=1 强制重装
 */
import { execSync, spawn } from 'node:child_process';
import { existsSync, readFileSync, statSync } from 'node:fs';
import { mkdir, writeFile, rm, readdir, unlink } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const DSH_VERSION = process.env.DSH_DESKTOP_DSH_VERSION ?? '0.1.0-rc.8';
const FORCE =
  process.argv.includes('--force') || process.env.DSH_DESKTOP_VENDOR_DSH_FORCE === '1';
const __dirname = path.dirname(fileURLToPath(import.meta.url));
const PLUGIN_DIR =
  process.env.DSH_DESKTOP_PLUGIN_DIR ??
  path.join(__dirname, '..', '..', 'plugins', 'dsh-usage-stats');
const DSH_DIR = path.join(__dirname, '..', 'src-tauri', 'resources', 'dsh');
const MANIFEST_PATH = path.join(DSH_DIR, 'dsh-manifest.json');
const BIN_JS = path.join(DSH_DIR, 'node_modules', '@deepseek-ai', 'dsh', 'lib', 'bin.js');
const DSH_PKG_JSON = path.join(DSH_DIR, 'node_modules', '@deepseek-ai', 'dsh', 'package.json');
const VENDORED_PLUGIN_LIB = path.join(
  DSH_DIR,
  'node_modules',
  'dsh-usage-stats',
  'lib',
  'index.js',
);

function readJson(p) {
  return JSON.parse(readFileSync(p, 'utf8'));
}

async function loadManifest() {
  if (!existsSync(MANIFEST_PATH)) return null;
  try {
    return readJson(MANIFEST_PATH);
  } catch {
    return null;
  }
}

/** 期望捆绑的插件状态(无插件目录 → 不捆绑) */
function expectedPluginState() {
  const pluginPkg = path.join(PLUGIN_DIR, 'package.json');
  if (!existsSync(pluginPkg)) {
    return { wanted: false, name: null, version: null, sourceMtime: null };
  }
  const pkg = readJson(pluginPkg);
  const pluginBin = path.join(PLUGIN_DIR, 'lib', 'index.js');
  const sourceMtime = existsSync(pluginBin) ? statSync(pluginBin).mtimeMs : null;
  return {
    wanted: true,
    name: pkg.name ?? 'dsh-usage-stats',
    version: pkg.version ?? null,
    sourceMtime,
  };
}

function installedDshVersion() {
  if (!existsSync(DSH_PKG_JSON)) return null;
  try {
    return readJson(DSH_PKG_JSON).version ?? null;
  } catch {
    return null;
  }
}

/** 本地安装树是否可复用 */
function isOutputCached(manifest, pluginState) {
  if (!manifest) return false;
  if (manifest.dshVersion !== DSH_VERSION) return false;
  if (!existsSync(BIN_JS)) return false;
  if (installedDshVersion() !== DSH_VERSION) return false;

  if (pluginState.wanted) {
    if (manifest.plugin !== pluginState.name) return false;
    if (!existsSync(VENDORED_PLUGIN_LIB)) return false;
    // 新 manifest 额外校验插件版本与源码 mtime;旧 manifest 无此字段时仍允许跳过
    if (manifest.pluginVersion != null && manifest.pluginVersion !== pluginState.version) {
      return false;
    }
    if (
      manifest.pluginSourceMtime != null &&
      manifest.pluginSourceMtime !== pluginState.sourceMtime
    ) {
      return false;
    }
  } else if (manifest.plugin != null) {
    return false;
  }

  return true;
}

async function pruneAppleDouble(dir) {
  if (process.platform === 'win32') return;
  let entries;
  try {
    entries = await readdir(dir, { withFileTypes: true });
  } catch {
    return;
  }
  for (const e of entries) {
    const p = path.join(dir, e.name);
    if (e.name.startsWith('._')) await unlink(p).catch(() => {});
    else if (e.isDirectory()) await pruneAppleDouble(p);
  }
}

async function vendorPlugin() {
  const pluginPkg = path.join(PLUGIN_DIR, 'package.json');
  if (!existsSync(pluginPkg)) {
    console.warn('[vendor-dsh] 未找到插件目录,跳过 dsh-usage-stats 捆绑');
    return false;
  }

  console.log(`[vendor-dsh] 捆绑插件 dsh-usage-stats <- ${PLUGIN_DIR}`);
  const pluginBin = path.join(PLUGIN_DIR, 'lib', 'index.js');
  if (!existsSync(pluginBin)) {
    console.log('[vendor-dsh]   插件缺 lib/ 构建产物,先执行构建…');
    const needsInstall = !existsSync(path.join(PLUGIN_DIR, 'node_modules', '.bin'));
    if (needsInstall) {
      console.log('[vendor-dsh]   插件依赖缺失,先 npm install…');
      execSync(`npm install --prefix "${PLUGIN_DIR}" --no-audit --no-fund`, {
        stdio: 'inherit',
        cwd: process.cwd(),
      });
    }
    execSync(`npm run build --prefix "${PLUGIN_DIR}"`, { stdio: 'inherit', cwd: process.cwd() });
    if (!existsSync(pluginBin)) {
      throw new Error(`插件构建后仍缺 ${pluginBin},请检查 ${PLUGIN_DIR} 的 build 脚本`);
    }
    console.log(`[vendor-dsh]   插件构建完成:${pluginBin}`);
  }

  const tmp = path.join(DSH_DIR, '.vendor-tmp');
  await mkdir(tmp, { recursive: true });
  const packed = execSync(`npm pack "${PLUGIN_DIR}" --pack-destination "${tmp}" --silent`, {
    encoding: 'utf8',
  }).trim();
  const tgz = path.join(tmp, packed);
  execSync(`npm install --prefix "${DSH_DIR}" "${tgz}" --omit=dev --no-audit --no-fund`, {
    stdio: 'inherit',
  });
  await rm(tmp, { recursive: true, force: true });

  if (!existsSync(VENDORED_PLUGIN_LIB)) {
    throw new Error(
      `插件打包产物缺 lib/index.js:${VENDORED_PLUGIN_LIB};请检查插件 package.json 的 files 声明`,
    );
  }
  return true;
}

async function runSmokeTest() {
  const nodeDir =
    process.platform === 'win32'
      ? 'win32-x64'
      : process.platform === 'darwin'
        ? process.arch === 'arm64'
          ? 'darwin-arm64'
          : 'darwin-x64'
        : 'linux-x64';
  const nodeBin = path.join(
    DSH_DIR,
    '..',
    'node',
    nodeDir,
    process.platform === 'win32' ? 'node.exe' : 'node',
  );
  if (!existsSync(nodeBin)) {
    console.warn('[vendor-dsh] 未找到 bundled node,跳过冒烟验证');
    return;
  }

  console.log('[vendor-dsh] 冒烟验证:启动 dsh web(--no-open)…');
  const smoke = spawn(nodeBin, [BIN_JS, 'web', '--port', '0', '--no-open'], {
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
}

async function writeManifest(pluginState, pluginVendored) {
  const manifest = {
    dshVersion: DSH_VERSION,
    bin: 'lib/bin.js',
    plugin: pluginVendored ? pluginState.name : null,
    pluginVersion: pluginVendored ? pluginState.version : null,
    pluginSourceMtime: pluginVendored ? pluginState.sourceMtime : null,
    pruned: true,
    installedAt: new Date().toISOString(),
  };
  await writeFile(MANIFEST_PATH, JSON.stringify(manifest, null, 2) + '\n');
}

async function main() {
  await mkdir(DSH_DIR, { recursive: true });
  const pluginState = expectedPluginState();
  const manifest = await loadManifest();

  if (!FORCE && isOutputCached(manifest, pluginState)) {
    await pruneAppleDouble(DSH_DIR);
    const pluginHint = pluginState.wanted ? ` + ${pluginState.name}@${pluginState.version}` : '';
    console.log(`[vendor-dsh] 已存在 @deepseek-ai/dsh@${DSH_VERSION}${pluginHint},跳过`);
    return;
  }

  if (FORCE) {
    console.log('[vendor-dsh] --force: 强制重新安装');
  }

  console.log(`[vendor-dsh] npm install @deepseek-ai/dsh@${DSH_VERSION} --omit=dev (may take a while)`);
  execSync(
    `npm install --prefix "${DSH_DIR}" @deepseek-ai/dsh@${DSH_VERSION} --omit=dev --no-audit --no-fund --no-package-lock=false`,
    { stdio: 'inherit', cwd: process.cwd() },
  );

  if (!existsSync(BIN_JS)) {
    throw new Error(`安装树不完整: 缺少 ${BIN_JS}`);
  }

  const pluginVendored = await vendorPlugin();

  console.log('[vendor-dsh] 保守裁剪依赖树(prune-dsh-tree.mjs)…');
  execSync(`node "${path.join(__dirname, 'prune-dsh-tree.mjs')}"`, {
    stdio: 'inherit',
    cwd: process.cwd(),
  });

  await runSmokeTest();
  await writeManifest(pluginState, pluginVendored);
  await pruneAppleDouble(DSH_DIR);
  console.log(`[vendor-dsh] 完成: ${DSH_DIR} (AppleDouble 侧车已清理)`);
}

main().catch((e) => {
  console.error('[vendor-dsh] 失败:', e.message);
  process.exit(1);
});
