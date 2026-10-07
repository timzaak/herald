import { readdir, writeFile } from "node:fs/promises";
import { join, relative } from "node:path";

const SITE_URL = process.env.SITE_URL || "https://auth.fornetcode.com";
const OUTPUT_DIR = ".output/public";

async function walk(dir) {
  const entries = await readdir(dir, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const fullPath = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === "assets" || entry.name === "__tsr") continue;
      files.push(...(await walk(fullPath)));
    } else if (entry.name === "index.html") {
      files.push(fullPath);
    }
  }
  return files;
}

function formatDate(date) {
  return date.toISOString().split("T")[0];
}

async function generateSitemap() {
  const htmlFiles = await walk(OUTPUT_DIR);
  const today = formatDate(new Date());

  const urls = htmlFiles
    .map((file) => {
      const rel = relative(OUTPUT_DIR, file);
      // /docs/getting-started/index.html → /docs/getting-started/
      const urlPath =
        "/" +
        rel
          .replace(/[/\\]index\.html$/, "")
          .replace(/index\.html$/, "")
          .replace(/\\/g, "/");
      return urlPath;
    })
    .sort()
    .map(
      (path) => `  <url>
    <loc>${SITE_URL}${path}</loc>
    <lastmod>${today}</lastmod>
  </url>`,
    )
    .join("\n");

  const sitemap = `<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
${urls}
</urlset>`;

  const outPath = join(OUTPUT_DIR, "sitemap.xml");
  await writeFile(outPath, sitemap, "utf-8");
  console.log(
    `Generated sitemap with ${urls.split("\n").length / 5} URLs → ${outPath}`,
  );
}

generateSitemap().catch((err) => {
  console.error("Failed to generate sitemap:", err);
  process.exit(1);
});
