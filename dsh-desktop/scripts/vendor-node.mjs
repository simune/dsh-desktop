#!/usr/bin/env node
/**
 * vendor-node.mjs —— 下载当前平台 Node 官方二进制到 src-tauri/resources/node/<platform>/。
 * 校验 SHASUMS256;仅保留二进制本体。对应 docs/03 §3.2。
 */
import { execFileSync } from 'node:child_process';
import { createWriteStream, existsSync, readFileSync } from 'node:fs';
import { mkdir, rm, writeFile, readFile, copyFile } from 'node:fs/promises';
import { pipeline } from 'node:stream/promises';
import https from 'node:https';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import crypto from 'node:crypto';

const NODE_VERSION = process.env.DSH_DESKTOP_NODE_VERSION ?? 'v22.23.2';
const __dirname = path.dirname(fileURLToPath(import.meta.url));
const RES = path.join(__dirname, '..', 'src-tauri', 'resources', 'node');

function distInfo() {
  const p = process.platform;
  const a = process.arch;
  if (p === 'darwin') {
    const sub = a === 'arm64' ? 'darwin-arm64' : 'darwin-x64';
    return { sub, tar: `node-${NODE_VERSION}-${sub}.tar.gz`, bin: 'node' };
  }
  if (p === 'win32') {
    // platformDir: 本地 resources 目录名(与 runtime.platform_dir() 一致)
    // distTag:    Node 官方发布包名(nodejs.org 用 win-x64 / win-arm64,不是 win32-)
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

async function main() {
  const info = distInfo();
  const { sub, tar, bin } = info;
  // Windows 官方 zip 内层目录用 distTag(win-x64);其它平台与 sub 相同
  const archiveRoot = info.distTag ?? sub;
  const base = `https://nodejs.org/dist/${NODE_VERSION}`;
  const tmp = path.join(RES, '.tmp');
  await mkdir(tmp, { recursive: true });

  // 1. 下载并校验
  const tarPath = path.join(tmp, tar);
  console.log(`[vendor-node] 下载 ${base}/${tar}`);
  await download(`${base}/${tar}`, tarPath);
  const shasums = await downloadText(`${base}/SHASUMS256.txt`);
  const expect = shasums.split('\n').find((l) => l.includes(tar))?.split(/\s+/)[0];
  if (!expect) throw new Error(`SHASUMS256.txt 中未找到 ${tar}`);
  const actual = sha256File(tarPath);
  if (actual !== expect) {
    throw new Error(`sha256 不匹配: ${tar}\n  期望 ${expect}\n  实际 ${actual}`);
  }
  console.log(`[vendor-node] sha256 校验通过`);

  // 2. 解压出二进制
  //    macOS/Linux: tar.gz/tar.xz,用系统 tar(bsdtar/GNU tar 均可,成员为 bin/<bin>)
  //    Windows:     zip 内部结构为 node-v<VER>-win-<arch>/node.exe(无 bin/ 目录),
  //                 用 Win10 1803+ 自带的 bsdtar 精确提取,无需安装 unzip
  const destDir = path.join(RES, sub);
  await mkdir(destDir, { recursive: true });
  const tmpExtract = path.join(tmp, 'extract');
  await rm(tmpExtract, { recursive: true, force: true });
  await mkdir(tmpExtract, { recursive: true });
  let extracted;
  if (process.platform === 'win32') {
    execFileSync('tar', ['-xf', tarPath, '-C', tmpExtract, `node-${NODE_VERSION}-${archiveRoot}/${bin}`]);
    extracted = path.join(tmpExtract, `node-${NODE_VERSION}-${archiveRoot}`, bin);
  } else {
    execFileSync('tar', ['-xzf', tarPath, '-C', tmpExtract, `node-${NODE_VERSION}-${archiveRoot}/bin/${bin}`]);
    extracted = path.join(tmpExtract, `node-${NODE_VERSION}-${archiveRoot}`, 'bin', bin);
  }
  const destBin = path.join(destDir, bin);
  await rm(destBin, { force: true });
  await copyFile(extracted, destBin);
  await rm(tmp, { recursive: true, force: true }); // 清理下载与解压残留
  await pruneAppleDouble(path.join(RES, sub));

  // 3. manifest
  const manifest = {
    nodeVersion: NODE_VERSION,
    platform: process.platform,
    arch: process.arch,
    nodeSha256: actual,
    builtAt: new Date().toISOString(),
  };
  await writeFile(path.join(RES, 'node-manifest.json'), JSON.stringify(manifest, null, 2) + '\n');
  console.log(`[vendor-node] 完成: ${destBin} (${manifest.nodeSha256.slice(0, 12)}…)`);
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

/** 清理 ExFAT 卷上的 AppleDouble 侧车(避免被打进安装包) */
async function pruneAppleDouble(dir) {
  const { readdir, unlink, stat } = await import('node:fs/promises');
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
