import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

// README 是整页文档，没有逐句回退：哪份漏改了，读者就看到一份半新半旧的页面。
// 这里拦住最容易漏的两类改动：章节结构和赞助商表。
//
// Web fork（cc-switch-web）差异：根 README.md 是 Web 版说明，上游多语言 README
// （README_ZH / README_JA / README_DE 等）已按 patches.md 的约定删除，中文说明
// 在 docs/README.zh-CN.md。因此这里只校验存在的两份 Web README：
// 语言切换器互链、章节结构一致；赞助商表是上游内容，Web 版没有，跳过。
const ROOT = path.resolve(__dirname, "../..");

const READMES = [
  ["README.md", "English"],
  ["docs/README.zh-CN.md", "中文"],
] as const;

const read = (file: string) => readFileSync(path.join(ROOT, file), "utf8");

const headingLevels = (text: string) =>
  [...text.matchAll(/^(#{1,4}) /gm)].map((m) => m[1].length);

const english = read("README.md");

describe.each(READMES)("%s", (file) => {
  it("exists", () => {
    expect(existsSync(path.join(ROOT, file))).toBe(true);
  });

  const text = existsSync(path.join(ROOT, file)) ? read(file) : "";

  it("keeps the same heading structure as the English README", () => {
    expect(headingLevels(text)).toEqual(headingLevels(english));
  });
});
