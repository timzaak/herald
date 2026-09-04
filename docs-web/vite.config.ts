import { readFileSync } from "node:fs";
import tailwindcss from "@tailwindcss/vite";
import { tanstackStart } from "@tanstack/react-start/plugin/vite";
import react from "@vitejs/plugin-react";
import mdx from "fumadocs-mdx/vite";
import { nitro } from "nitro/vite";
import { defineConfig } from "vite";

// OpenAPI pages are virtual (one per operationId in the spec, grouped by tag).
// The prerender crawler can't discover them from virtual pages, so we list them
// explicitly — one per operationId, in both locales. Generated from openapi.json
// so it stays in sync when the backend adds/removes endpoints.
//
// `groupBy: "tag"` (src/lib/source.ts) nests each page under its tag folder,
// so the path is `/openapi/<tag>/<operationId>`. fumadocs-openapi slugifies the
// tag with `(s) => s.replace(/\s+/g, "-").toLowerCase()` — since our tags have
// no spaces, the lowercase tag is the folder segment verbatim.
function openapiPages() {
  try {
    const spec = JSON.parse(readFileSync("./openapi.json", "utf-8")) as {
      paths?: Record<
        string,
        Record<string, { operationId?: string; tags?: string[] }>
      >;
    };
    const seen = new Set<string>();
    const pages: { path: string }[] = [];
    for (const ops of Object.values(spec.paths ?? {})) {
      for (const op of Object.values(ops)) {
        if (!op.operationId || !op.tags?.[0]) continue;
        const tag = op.tags[0].toLowerCase();
        const id = op.operationId;
        const key = `${tag}/${id}`;
        if (seen.has(key)) continue;
        seen.add(key);
        pages.push({ path: `/en/docs/openapi/${tag}/${id}` });
        pages.push({ path: `/zh/docs/openapi/${tag}/${id}` });
      }
    }
    return pages.sort((a, b) => a.path.localeCompare(b.path));
  } catch {
    return [];
  }
}

export default defineConfig({
  server: {
    port: 3001,
  },
  plugins: [
    mdx(),
    tailwindcss(),
    tanstackStart({
      spa: {
        enabled: true,
        prerender: {
          enabled: true,
          crawlLinks: true,
        },
      },

      // Prerender tuning for ~200 virtual OpenAPI pages.
      // Defaults: concurrency = os.cpus().length (too high — overwhelms the
      // localhost prerender server → ETIMEDOUT) and retryCount = 0 (a single
      // timeout drops the page silently). Lower concurrency to ease the server
      // and retry transient timeouts so no page is silently lost.
      prerender: {
        concurrency: 3,
        retryCount: 3,
        retryDelay: 1000,
      },

      pages: [
        {
          path: "/docs",
        },
        {
          path: "/en/docs",
        },
        {
          path: "/zh/docs",
        },
        // Locale root pages
        {
          path: "/en",
        },
        {
          path: "/zh",
        },
        // Blog listing — post pages are crawled from links on the listing
        {
          path: "/blog",
        },
        // Legal pages
        {
          path: "/en/privacy",
        },
        {
          path: "/zh/privacy",
        },
        {
          path: "/en/terms",
        },
        {
          path: "/zh/terms",
        },
        // OpenAPI tag pages — virtual, must be listed explicitly (crawler can't render them)
        ...openapiPages(),
        {
          path: "/api/search",
        },
        {
          path: "llms-full.txt",
        },
        {
          path: "llms.txt",
        },
      ],
    }),
    react(),
    // please see https://tanstack.com/start/latest/docs/framework/react/guide/hosting#nitro for guides on hosting
    nitro(),
  ],
  resolve: {
    tsconfigPaths: true,
  },
});
