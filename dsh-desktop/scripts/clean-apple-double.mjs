#!/usr/bin/env node
/**
 * clean-apple-double.mjs —— 跨平台清理 AppleDouble 侧车文件(._*)。
 * 原 package.json 的 `find . -name '._*' -delete` 依赖 unix find,
 * Windows 无此命令;本脚本递归删除以 ._ 开头的文件,跳过构建/依赖目录。
 */
import { readdir, unlink } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, '..');
const SKIP = new Set(['node_modules', '.git', 'target', 'dist']);

let removed = 0;
async function walk(dir) {
  let entries;
  try {
    entries = await readdir(dir, { withFileTypes: true });
  } catch {
    return;
  }
  for (const e of entries) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) {
      if (!SKIP.has(e.name)) await walk(p);
    } else if (e.name.startsWith('._')) {
      await unlink(p).catch(() => {});
      removed++;
    }
  }
}

await walk(ROOT);
console.log(`[clean-apple-double] 清理 ${removed} 个 AppleDouble 侧车文件`);
