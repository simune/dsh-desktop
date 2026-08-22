#!/usr/bin/env node
/**
 * vendor-node.mjs —— 下载当前平台 Node 官方二进制到 src-tauri/resources/node/<platform>/。
 * 校验 SHASUMS256;仅保留二进制本体。对应 docs/03 §3.2。
 *
 * 缓存策略:
 * 1. 若 node-manifest + 目标二进制已存在且 sha256 匹配 → 整步跳过
 * 2. 否则若本地归档缓存命中 → 跳过下载,仅解压
 * 3. --force 或 DSH_DESKTOP_VENDOR_NODE_FORCE=1 强制重新下载
 */
import { execFileSync } from 'node:child_process';
import { createWriteStream, existsSync, readFileSync } from 'node:fs';
import { mkdir, rm, writeFile, readFile, copyFile } from 'node:fs/promises';
import { pipeline } from 'node:stream/promises';
import https from 'node:https';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import crypto from 'node:crypto';

const NODE_VERSION = process.env.DSH_DESKTOP_NODE_VERSION ?? 'v22.23.2';
const FORCE =
  process.argv.includes('--force') || process.env.DSH_DESKTOP_VENDOR_NODE_FORCE === '1';
const __dirname = path.dirname(fileURLToPath(import.meta.url));
const RES = path.join(__dirname, '..', 'src-tauri', 'resources', 'node');
const MANIFEST_PATH = path.join(RES, 'node-manifest.json');

function distInfo() {
  const p = process.platform;
  const a = process.arch;
  if (p === 'darwin') {
    const sub = a === 'arm64' ? 'darwin-arm64' : 'darwin-x64';
    return { sub, tar: `node-${NODE_VERSION}-${sub}.tar.gz`, bin: 'node' };
  }
  if (p === 'win32') {
    const platformDir = a === 'arm64' ? 'win32-arm64' : 'win32-x64';
    const distTag = a === 'arm64' ? 'win-arm64' : 'win-x64';
    return {
      sub: platformDir,
      distTag,
      tar: `node-${NODE_VERSION}-${distTag}.zip`,
      bin: 'node.exe',
    };
  }
  const sub = a === 'arm64' ? 'linux-arm64' : 'linux-x64';
  return { sub, tar: `node-${NODE_VERSION}-${sub}.tar.xz`, bin: 'node' };
}

function archiveCacheDir() {
  if (process.platform === 'darwin') {
    return path.join(os.homedir(), 'Library', 'Caches', 'dsh-desktop', 'node-archives');
  }
  if (process.platform === 'win32') {
    const base = process.env.LOCALAPPDATA || path.join(os.homedir(), 'AppData', 'Local');
    return path.join(base, 'dsh-desktop', 'node-archives');
  }
  return path.join(os.homedir(), '.cache', 'dsh-desktop', 'node-archives');
}

function download(url, dest) {
  return new Promise((resolve, reject) => {
    https.get(url, (res) => {
      if (res.statusCode !== 200) {
        reject(new Error(`HTTP ${res.statusCode} for ${url}`));
        res.resume();
        return;
      }
      pipeline(res, createWriteStream(dest)).then(resolve).catch(reject);
    }).on('error', reject);
  });
}

function downloadText(url) {
  return new Promise((resolve, reject) => {
    https.get(url, (res) => {
      if (res.statusCode !== 200) return reject(new Error(`HTTP ${res.statusCode}`));
      let s = '';
      res.setEncoding('utf8');
      res.on('data', (d) => (s += d));
      res.on('end', () => resolve(s));
    }).on('error', reject);
  });
}

function sha256File(p) {
  return crypto.createHash('sha256').update(readFileSync(p)).digest('hex');
}

async function loadManifest() {
  if (!existsSync(MANIFEST_PATH)) return null;
  try {
    return JSON.parse(await readFile(MANIFEST_PATH, 'utf8'));
  } catch {
    return null;
  }
}

async function expectedArchiveSha256(base, tar) {
  const shasums = await downloadText(`${base}/SHASUMS256.txt`);
  const expect = shasums.split('\n').find((l) => l.includes(tar))?.split(/\s+/)[0];
  if (!expect) throw new Error(`SHASUMS256.txt 中未找到 ${tar}`);
  return expect;
}

/** 已 vendored 的二进制是否仍有效 */
function isOutputCached(manifest, destBin) {
  if (!manifest || !existsSync(destBin)) return false;
  if (manifest.nodeVersion !== NODE_VERSION) return false;
  if (manifest.platform !== process.platform) return false;
  if (manifest.arch !== process.arch) return false;
  if (!manifest.nodeSha256) return false;
  return sha256File(destBin) === manifest.nodeSha256;
}

/** 获取可用归档:优先本地缓存,否则下载并写入缓存 */
async function resolveArchive(base, tar) {
  const expect = await expectedArchiveSha256(base, tar);
  const cacheDir = archiveCacheDir();
  await mkdir(cacheDir, { recursive: true });
  const cached = path.join(cacheDir, tar);

  if (!FORCE && existsSync(cached) && sha256File(cached) === expect) {
    console.log(`[vendor-node] 使用归档缓存: ${cached}`);
    return { tarPath: cached, archiveSha256: expect };
  }

  const tmp = path.join(cacheDir, `.download-${tar}`);
  console.log(`[vendor-node] 下载 ${base}/${tar}`);
  await download(`${base}/${tar}`, tmp);
  const actual = sha256File(tmp);
  if (actual !== expect) {
    await rm(tmp, { force: true });
    throw new Error(`sha256 不匹配: ${tar}\n  期望 ${expect}\n  实际 ${actual}`);
  }
  await rm(cached, { force: true });
  await copyFile(tmp, cached);
  await rm(tmp, { force: true });
  console.log(`[vendor-node] sha256 校验通过,已写入归档缓存`);
  return { tarPath: cached, archiveSha256: expect };
}

async function main() {
  const info = distInfo();
  const { sub, tar, bin } = info;
  const archiveRoot = info.distTag ?? sub;
  const base = `https://nodejs.org/dist/${NODE_VERSION}`;
  const destDir = path.join(RES, sub);
  const destBin = path.join(destDir, bin);

  const manifest = await loadManifest();
  if (!FORCE && isOutputCached(manifest, destBin)) {
    await pruneAppleDouble(destDir);
    console.log(
      `[vendor-node] 已存在 ${NODE_VERSION} (${sub}),跳过 (${manifest.nodeSha256.slice(0, 12)}…)`,
    );
    return;
  }

  if (FORCE) {
    console.log('[vendor-node] --force: 强制重新下载');
  }

  const { tarPath } = await resolveArchive(base, tar);

  // 解压出二进制
  const tmp = path.join(RES, '.tmp');
  await mkdir(tmp, { recursive: true });
  const tmpExtract = path.join(tmp, 'extract');
  await rm(tmpExtract, { recursive: true, force: true });
  await mkdir(tmpExtract, { recursive: true });

  let extracted;
  if (process.platform === 'win32') {
    execFileSync('tar', ['-xf', tarPath, '-C', tmpExtract, `node-${NODE_VERSION}-${archiveRoot}/${bin}`]);
    extracted = path.join(tmpExtract, `node-${NODE_VERSION}-${archiveRoot}`, bin);
  } else {
    const member = `node-${NODE_VERSION}-${archiveRoot}/bin/${bin}`;
    if (tar.endsWith('.tar.xz')) {
      execFileSync('tar', ['-xJf', tarPath, '-C', tmpExtract, member]);
    } else {
      execFileSync('tar', ['-xzf', tarPath, '-C', tmpExtract, member]);
    }
    extracted = path.join(tmpExtract, `node-${NODE_VERSION}-${archiveRoot}`, 'bin', bin);
  }

  await mkdir(destDir, { recursive: true });
  await rm(destBin, { force: true });
  await copyFile(extracted, destBin);
  await rm(tmp, { recursive: true, force: true });
  await pruneAppleDouble(destDir);

  const nodeSha256 = sha256File(destBin);
  const nextManifest = {
    nodeVersion: NODE_VERSION,
    platform: process.platform,
    arch: process.arch,
    platformDir: sub,
    nodeSha256,
    builtAt: new Date().toISOString(),
  };
  await writeFile(MANIFEST_PATH, JSON.stringify(nextManifest, null, 2) + '\n');
  console.log(`[vendor-node] 完成: ${destBin} (${nodeSha256.slice(0, 12)}…)`);
}

/** 清理 ExFAT 卷上的 AppleDouble 侧车(避免被打进安装包) */
async function pruneAppleDouble(dir) {
  const { readdir, unlink } = await import('node:fs/promises');
  const entries = await readdir(dir, { withFileTypes: true });
  for (const e of entries) {
    const p = path.join(dir, e.name);
    if (e.name.startsWith('._')) await unlink(p);
    else if (e.isDirectory()) await pruneAppleDouble(p);
  }
}

main().catch((e) => {
  console.error('[vendor-node] 失败:', e.message);
  process.exit(1);
});
