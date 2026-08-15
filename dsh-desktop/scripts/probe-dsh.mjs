#!/usr/bin/env node
/**
 * probe-dsh.mjs —— M0.2 原型 + 回归工具:
 * spawn 一个 dsh web(--port 0),逐行解析 stdout 提取 URL,TCP 健康检查,退出时清理。
 *
 * 用法:
 *   node probe-dsh.mjs                     # 默认:真实 dsh
 *   node probe-dsh.mjs --fake [fakeArgs]   # 用 fake-dsh.mjs 代替(测解析,如 --lan/--noise)
 *   node probe-dsh.mjs --bin <path>        # 指定 dsh bin.js
 *
 * 退出码: 0 = 解析到 URL 且 TCP 连通;1 = 失败。
 */
import { spawn } from 'node:child_process';
import net from 'node:net';
import readline from 'node:readline';
import path from 'node:path';
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const URL_RE = /^dsh web: (http:\/\/127\.0\.0\.1:\d+)/;
const START_TIMEOUT_MS = 60_000;

/** 默认 dsh bin.js:macOS homebrew 路径;Windows 探测 npm 全局布局 */
function defaultDshBin() {
  if (process.platform === 'win32') {
    const roots = [];
    if (process.env.APPDATA) roots.push(path.join(process.env.APPDATA, 'npm'));
    if (process.env.ProgramFiles) roots.push(path.join(process.env.ProgramFiles, 'nodejs'));
    if (process.env.LOCALAPPDATA) roots.push(path.join(process.env.LOCALAPPDATA, 'fnm_multishells', 'current')); // fnm 场景尽力而为
    if (roots.length === 0) return '';
    for (const r of roots) {
      const bin = path.join(r, 'node_modules', '@deepseek-ai', 'dsh', 'lib', 'bin.js');
      if (existsSync(bin)) return bin;
    }
    return path.join(roots[0] ?? '', 'node_modules', '@deepseek-ai', 'dsh', 'lib', 'bin.js');
  }
  return '/opt/homebrew/lib/node_modules/@deepseek-ai/dsh/lib/bin.js';
}

function parseUrl(line) {
  const m = URL_RE.exec(line);
  return m ? m[1] : null;
}

function tcpCheck(host, port, timeoutMs) {
  return new Promise((resolve) => {
    const sock = net.connect({ host, port });
    const timer = setTimeout(() => { sock.destroy(); resolve(false); }, timeoutMs);
    sock.once('connect', () => { clearTimeout(timer); sock.end(); resolve(true); });
    sock.once('error', () => { clearTimeout(timer); resolve(false); });
  });
}

async function main() {
  const args = process.argv.slice(2);
  let cmd, cmdArgs;
  if (args[0] === '--fake') {
    cmd = process.execPath;
    cmdArgs = [path.join(__dirname, 'fake-dsh.mjs'), ...args.slice(1)];
  } else {
    const binIdx = args.indexOf('--bin');
    const bin = binIdx !== -1 ? args[binIdx + 1] : defaultDshBin();
    cmd = process.execPath;
    cmdArgs = [bin, 'web', '--port', '0'];
  }

  const child = spawn(cmd, cmdArgs, { stdio: ['ignore', 'pipe', 'pipe'] });
  const outLog = [], errLog = [];
  const rl = readline.createInterface({ input: child.stdout });
  let url = null;

  rl.on('line', (line) => {
    outLog.push(line);
    if (!url) url = parseUrl(line);
  });
  child.stderr.on('data', (d) => errLog.push(d.toString()));

  const deadline = Date.now() + START_TIMEOUT_MS;
  let exitCode = 0;
  while (!url && Date.now() < deadline) {
    const st = child.exitCode ?? (child.exitCode === null ? null : null);
    if (child.exitCode !== null || child.signalCode !== null) break;
    await new Promise((r) => setTimeout(r, 100));
  }

  if (url) {
    const [, port] = /:(\d+)$/.exec(url);
    const ok = await tcpCheck('127.0.0.1', Number(port), 3000);
    console.log(`[probe] url=${url} tcp=${ok ? 'ok' : 'FAIL'}`);
    if (!ok) exitCode = 1;
  } else {
    console.error('[probe] FAIL: 未在超时内解析到 URL 行');
    console.error('[probe] stdout tail:', outLog.slice(-10).join('\n'));
    console.error('[probe] stderr tail:', errLog.slice(-20).join(''));
    exitCode = 1;
  }

  // 清理:Windows 用 taskkill /T /F(与主 app 一致);其它平台 SIGTERM → 2s → SIGKILL
  if (child.exitCode === null && child.signalCode === null) {
    if (process.platform === 'win32' && child.pid) {
      try {
        spawn('taskkill', ['/T', '/F', '/PID', String(child.pid)], {
          stdio: 'ignore',
          windowsHide: true,
        });
      } catch {
        child.kill();
      }
      await new Promise((r) => setTimeout(r, 2000));
    } else {
      child.kill('SIGTERM');
      await new Promise((r) => setTimeout(r, 2000));
      if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL');
    }
  }
  process.exit(exitCode);
}

main();
