#!/usr/bin/env node
/**
 * fake-dsh.mjs —— 模拟 dsh web 子进程,用于解析/健康检查/重启逻辑的集成测试。
 * 用法:
 *   node fake-dsh.mjs [--noise <n>] [--lan] [--delay <ms>] [--serve] [--crash-after <ms>] [--no-url]
 * 行为: 打印 n 行噪声 → (可选 --serve: 起本地 HTTP 服务) → 打印 "dsh web: http://127.0.0.1:<port>"
 *       --lan: 行尾追加 " (LAN: http://192.168.1.5:<port>)"
 *       --crash-after: 打印 URL 后 ms 毫秒退出(码 1)
 *       --no-url: 永不打印 URL(测超时)
 */
import http from 'node:http';

const args = process.argv.slice(2);
const opt = (name, dflt) => {
  const i = args.indexOf(name);
  return i === -1 ? dflt : (args[i + 1] ?? dflt);
};
const noise = Number(opt('--noise', 0));
const lan = args.includes('--lan');
const serve = args.includes('--serve');
const noUrl = args.includes('--no-url');
const delay = Number(opt('--delay', 0));
const crashAfter = Number(opt('--crash-after', 0));

let port = 0;
if (serve) {
  const srv = http.createServer((req, res) => {
    res.writeHead(200, { 'content-type': 'text/html' });
    res.end('<html><head><title>fake-dsh</title></head><body>ok</body></html>');
  });
  await new Promise((res) => srv.listen(0, '127.0.0.1', res));
  port = srv.address().port;
}

for (let i = 0; i < noise; i++) console.log(`noise line ${i} (unrelated output)`);
if (delay > 0) await new Promise((r) => setTimeout(r, delay));
if (!noUrl) {
  const url = `http://127.0.0.1:${port || 12345}`;
  console.log(`dsh web: ${url}${lan ? ` (LAN: http://192.168.1.5:${port || 12345})` : ''}`);
}
if (crashAfter > 0) setTimeout(() => process.exit(1), crashAfter);

process.on('SIGTERM', () => process.exit(0));
process.on('SIGINT', () => process.exit(0));
// 保活:定时器占住事件循环(Node 22 对未决 top-level await 会在循环空时以 code 13 退出)
setInterval(() => {}, 1 << 30);
