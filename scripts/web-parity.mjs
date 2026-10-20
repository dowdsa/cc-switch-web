#!/usr/bin/env node
// 对比前端实际 invoke 的命令与 Web 端已实现的 RPC/覆盖命令。
// 用法: node scripts/web-parity.mjs [--strict]
import { readFileSync, readdirSync, statSync } from "node:fs";
import path from "node:path";

const root = process.cwd();
const strict = process.argv.includes("--strict");

function walk(dir) {
  const entries = [];
  for (const name of readdirSync(dir)) {
    const full = path.join(dir, name);
    const stat = statSync(full);
    if (stat.isDirectory()) entries.push(...walk(full));
    else entries.push(full);
  }
  return entries;
}

const invokePattern =
  /invoke(?:<[^>]*>)?\(\s*["'`]([a-z0-9_]+)["'`]/g;
const frontendCommands = new Map();
for (const file of walk(path.join(root, "src"))) {
  if (!file.endsWith(".ts") && !file.endsWith(".tsx")) continue;
  const text = readFileSync(file, "utf8");
  for (const match of text.matchAll(invokePattern)) {
    const name = match[1];
    const list = frontendCommands.get(name) ?? [];
    list.push(path.relative(root, file));
    frontendCommands.set(name, list);
  }
}

const rpcCommands = new Set();
for (const file of walk(path.join(root, "src-tauri", "src", "web"))) {
  if (!file.endsWith(".rs")) continue;
  const text = readFileSync(file, "utf8");
  for (const match of text.matchAll(/^\s*((?:"[a-z0-9_]+"\s*\|\s*)*"[a-z0-9_]+")\s*=>/gm)) {
    for (const name of match[1].matchAll(/"([a-z0-9_]+)"/g)) {
      rpcCommands.add(name[1]);
    }
  }
  // 在 server.rs 中特殊分发的命令（二进制/流式）
  for (const match of text.matchAll(/body\.cmd\s*[!=]=\s*"([a-z0-9_]+)"/g)) {
    rpcCommands.add(match[1]);
  }
}

const overrides = new Set();
const overrideFile = path.join(
  root,
  "src",
  "web",
  "shims",
  "overrides.ts",
);
const overrideText = readFileSync(overrideFile, "utf8");
for (const match of overrideText.matchAll(/case\s+"([a-z0-9_]+)":/g)) {
  overrides.add(match[1]);
}

const missing = [...frontendCommands.keys()]
  .filter((name) => !rpcCommands.has(name) && !overrides.has(name))
  .sort();

console.log(`前端调用命令: ${frontendCommands.size}`);
console.log(`Web RPC 实现: ${rpcCommands.size}`);
console.log(`浏览器覆盖/空操作: ${overrides.size}`);
console.log(`未覆盖: ${missing.length}`);
if (missing.length > 0) {
  console.log("\n未覆盖命令:");
  for (const name of missing) {
    console.log(`  - ${name}`);
  }
}

if (strict && missing.length > 0) {
  process.exitCode = 1;
}
