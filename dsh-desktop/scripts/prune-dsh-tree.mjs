#!/usr/bin/env node
/**
 * prune-dsh-tree.mjs —— 保守裁剪 bundled dsh 依赖树,减少打包文件数(NSIS 解包提速)。
 *
 * 原则(用户约定):只删"确定运行时不需要"的文件,不确定的一律保留,避免改坏稳定版。
 *
 * 保守删除清单:
 *   1. 包内元文件:README / CHANGELOG / LICENSE / COPYING / NOTICE / AUTHORS / CONTRIBUTING /
 *                 SECURITY / CODE_OF_CONDUCT / SUPPORT / FUNDING (含 .md/.txt/.rst/.markdown)
 *   2. sourcemap:*.map (仅 devtools 使用,node 运行时绝不 require)
 *   3. 测试/文档目录:test/ tests/ __tests__/ .github/ .vscode/ docs/ (仅当目录名精确匹配)
 *
 * 保护规则:
 *   - @deepseek-ai/dsh 主包目录整体跳过(含 bin.js 入口,最保守)
 *   - 顶层目录 node_modules 自身不动
 *
 * 用法:
 *   node scripts/prune-dsh-tree.mjs              # 真实删除
 *   node scripts/prune-dsh-tree.mjs --dry-run    # 只统计,不删除
 *   env PRUNE_DSH_DIR=<dir> node ...             # 指定目标树(默认 resources/dsh)
 *
 * 退出码:0 成功;非 0 失败。
 */
import { readdir, rm, stat } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const DSH_DIR = process.env.PRUNE_DSH_DIR ?? path.join(__dirname, '..', 'src-tauri', 'resources', 'dsh');
const DRY_RUN = process.argv.includes('--dry-run');

// 文件名匹配(不区分大小写,去掉扩展名后匹配前缀)
const META_NAME_RE = /^(readme|changelog|license|copying|notice|authors|contributing|security|code_of_conduct|support|funding|privacy)(\.|$)/i;
const META_EXT_RE = /\.(md|markdown|txt|rst)$/i;
const TEST_DIR_RE = /^(test|tests|__tests__|\.github|\.vscode|docs)$/i;
const SKIP_DIRS = new Set(['node_modules', '.bin', '.package-lock.json']); // .package-lock.json 是文件,见下

const PROTECTED_PACKAGE = 'dsh'; // @deepseek-ai/dsh 主包名

let removedFiles = 0;
let removedDirs = 0;
let removedBytes = 0;
let scannedFiles = 0;

function isMetaFile(name) {
  // 形如 README.md / LICENSE / CHANGELOG 等
  if (!META_NAME_RE.test(name)) return false;
  // 只删文档类扩展名;遇到名称很像但扩展名不确定的(如 LICENSE.custom)保守保留
  if (META_EXT_RE.test(name)) return true;
  // 无扩展名的(如 LICENSE、NOTICE)也删——它们是纯文本说明
  if (!path.extname(name)) return true;
  return false;
}

async function pruneDir(dir, isPackageRoot) {
  let entries;
  try {
    entries = await readdir(dir, { withFileTypes: true });
  } catch {
    return;
  }
  for (const e of entries) {
    const full = path.join(dir, e.name);
    if (e.isDirectory()) {
      // 包根下的 test/docs 目录才删;node_modules 深层不处理目录级(保守)
      if (isPackageRoot && TEST_DIR_RE.test(e.name)) {
        removedDirs++;
        if (DRY_RUN) {
          console.log(`[prune] (dry) dir  ${path.relative(DSH_DIR, full)}`);
        } else {
          await rm(full, { recursive: true, force: true });
        }
        continue;
      }
      await pruneDir(full, false);
    } else if (e.isFile()) {
      scannedFiles++;
      // sourcemap:任何位置的 *.map 都删(运行时绝不读取)
      if (e.name.endsWith('.map')) {
        removedFiles++;
        removedBytes += (await safeSize(full));
        if (DRY_RUN) console.log(`[prune] (dry) map  ${path.relative(DSH_DIR, full)}`);
        else await rm(full, { force: true });
        continue;
      }
      // 元文件:仅在包根一级删(保守,不在深层目录乱删)
      if (isPackageRoot && isMetaFile(e.name)) {
        removedFiles++;
        removedBytes += (await safeSize(full));
        if (DRY_RUN) console.log(`[prune] (dry) meta ${path.relative(DSH_DIR, full)}`);
        else await rm(full, { force: true });
      }
    }
  }
}

async function safeSize(p) {
  try {
    return (await stat(p)).size;
  } catch {
    return 0;
  }
}

async function main() {
  const nmDir = path.join(DSH_DIR, 'node_modules');
  if (!existsSync(nmDir)) {
    console.error(`[prune] 未找到 ${nmDir},退出`);
    process.exit(1);
  }

  // 顶层包目录(@scope/name 两级)
  const scoped = await readdir(nmDir, { withFileTypes: true }).catch(() => []);
  for (const entry of scoped) {
    if (!entry.isDirectory()) continue;
    const firstLevel = path.join(nmDir, entry.name);
    if (entry.name.startsWith('@')) {
      // scoped:@scope/name
      const inner = await readdir(firstLevel, { withFileTypes: true }).catch(() => []);
      for (const sub of inner) {
        if (!sub.isDirectory()) continue;
        const pkgDir = path.join(firstLevel, sub.name);
        if (sub.name === PROTECTED_PACKAGE) {
          console.log(`[prune] 跳过保护包 @deepseek-ai/${PROTECTED_PACKAGE}`);
          continue;
        }
        await pruneDir(pkgDir, true);
      }
    } else {
      // 普通包
      if (entry.name === PROTECTED_PACKAGE) {
        console.log(`[prune] 跳过保护包 ${PROTECTED_PACKAGE}`);
        continue;
      }
      await pruneDir(firstLevel, true);
    }
  }

  console.log(`[prune] ${DRY_RUN ? '(dry-run) ' : ''}完成:`);
  console.log(`[prune]   扫描文件: ${scannedFiles}`);
  console.log(`[prune]   删除文件: ${removedFiles} (${(removedBytes / 1024 / 1024).toFixed(1)} MB)`);
  console.log(`[prune]   删除目录: ${removedDirs}`);
}

main().catch((e) => {
  console.error('[prune] 失败:', e.message);
  process.exit(1);
});
