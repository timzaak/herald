// One-off internal-link audit for docs-web content.
// Usage: node scripts/link-audit.mjs  (from docs-web/)
// Checks every markdown link ]](/docs...) : does the target .mdx exist,
// and does the #anchor match a heading slug in that file?
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, dirname } from "node:path";
import GithubSlugger from "github-slugger";

const CONTENT = "content/docs";

function walk(dir) {
  const out = [];
  for (const entry of readdirSync(dir)) {
    const p = join(dir, entry);
    if (statSync(p).isDirectory()) out.push(...walk(p));
    else if (p.endsWith(".mdx")) out.push(p);
  }
  return out;
}

const files = walk(CONTENT);
// Pages generated at build time (fumadocs-openapi), not backed by an .mdx file.
const GENERATED = new Set(["docs/openapi"]);
const headingsByFile = new Map(); // "docs/zh/configuration" -> Set of slugs
for (const f of files) {
  // key matches link path minus leading slash: content/docs/zh/x.mdx -> docs/zh/x
  const key = f.replace(/\\/g, "/").replace(/^content\//, "").replace(/\.mdx$/, "");
  const slugger = new GithubSlugger();
  const slugs = new Set();
  let inFence = false;
  for (const line of readFileSync(f, "utf8").split("\n")) {
    if (line.trimStart().startsWith("```")) inFence = !inFence;
    if (inFence) continue;
    const m = line.match(/^(#{1,6})\s+(.*?)\s*\r?$/);
    if (m) slugs.add(slugger.slug(m[2].trim()));
  }
  headingsByFile.set(key, slugs);
}

const linkRe = /\]\((\/(?:zh\/)?docs[^)\s]*)\)/g;
const problems = [];
let checked = 0;
for (const f of files) {
  let inFence = false;
  const text = readFileSync(f, "utf8");
  const lines = text.split("\n");
  for (let i = 0; i < lines.length; i++) {
    if (lines[i].trimStart().startsWith("```")) inFence = !inFence;
    if (inFence) continue;
    for (const m of lines[i].matchAll(linkRe)) {
      checked++;
      const url = m[1].replace(/\r$/, "");
      const [path, anchor] = url.split("#");
      // /zh/docs/x and /docs/zh/x are the same page; normalize to the file key.
      const key = path.replace(/^\//, "").replace(/^zh\/docs\//, "docs/zh/");
      if (!headingsByFile.has(key) && !GENERATED.has(key)) {
        problems.push(`${f}:${i + 1} NO TARGET FILE -> ${url}`);
        continue;
      }
      if (anchor && headingsByFile.has(key)) {
        const decoded = decodeURIComponent(anchor);
        if (!headingsByFile.get(key).has(decoded)) {
          problems.push(`${f}:${i + 1} BAD ANCHOR -> ${url}`);
        }
      }
    }
  }
}
console.log(`checked ${checked} internal links in ${files.length} files`);
console.log(problems.length ? problems.join("\n") : "all links resolve");
