import { blog, docs } from "collections/server";
import { loader, multiple } from "fumadocs-core/source";
import { lucideIconsPlugin } from "fumadocs-core/source/lucide-icons";
import { toFumadocsSource } from "fumadocs-mdx/runtime/server";
import { openapiPlugin, openapiSource } from "fumadocs-openapi/server";
import { i18n } from "./i18n";
import { openapi } from "./openapi";

const docsRoute = "/docs";
const blogRoute = "/blog";

export const source = loader(
  multiple({
    docs: docs.toFumadocsSource(),
    openapi: await openapiSource(openapi, {
      baseDir: "openapi",
      groupBy: "tag",
    }),
  }),
  {
    baseUrl: docsRoute,
    i18n,
    url(slugs, locale) {
      const loc = locale || i18n.defaultLanguage;
      return `/${[loc, "docs", ...slugs.filter(Boolean)].join("/")}`;
    },
    plugins: [lucideIconsPlugin(), openapiPlugin()],
  },
);

// `create.doc` collections are plain page arrays — wrap with the standalone
// toFumadocsSource (no meta files, blog has no sidebar tree). Blog is
// single-language English, so no i18n: URLs are /blog/<slug>.
export const blogSource = loader(toFumadocsSource(blog, []), {
  baseUrl: blogRoute,
  plugins: [lucideIconsPlugin()],
});

export function markdownPathToSlugs(segs: string[]) {
  if (segs.length === 0) return [];

  const out = [...segs];
  out[out.length - 1] = out[out.length - 1].replace(/\.md$/, "");
  if (out.length === 1 && out[0] === "index") out.pop();
  return out;
}

export function slugsToMarkdownPath(slugs: string[], locale?: string) {
  const segments = [...slugs];
  if (segments.length === 0) {
    segments.push("index.md");
  } else {
    segments[segments.length - 1] += ".md";
  }

  const base = locale ? `/${locale}${docsRoute}` : docsRoute;
  return {
    segments,
    url: `${base}/${segments.join("/")}`,
  };
}

export function getPageMarkdownUrl(slugs: string[]) {
  const segments = [...slugs];
  if (segments.length === 0) {
    segments.push("index.md");
  } else {
    segments[segments.length - 1] += ".md";
  }

  return {
    segments,
    url: `${docsRoute}/${segments.join("/")}`,
  };
}

export async function getLLMText(page: (typeof source)["$inferPage"]) {
  if (page.data.type === "openapi") {
    // With 170+ OpenAPI pages, dumping each full schema into llms-full.txt
    // overflows the V8 string limit. Emit a compact per-operation summary
    // instead; the full schema stays available on each page's HTML.
    const schema = page.data.getSchema() as { paths?: Record<string, unknown> };
    const paths = schema.paths ?? {};
    const lines: string[] = [`# ${page.data.title} (${page.url})`, ""];
    for (const [path, ops] of Object.entries(paths)) {
      for (const [method, op] of Object.entries(ops as Record<string, unknown>)) {
        const summary = (op as { summary?: string }).summary ?? "";
        lines.push(`- ${method.toUpperCase()} ${path} — ${summary}`);
      }
    }
    return lines.join("\n");
  }

  const processed = await page.data.getText("processed");

  return `# ${page.data.title} (${page.url})

${processed}`;
}
