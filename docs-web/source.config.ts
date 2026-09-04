import { remarkMdxMermaid } from "fumadocs-core/mdx-plugins";
import { pageSchema } from "fumadocs-core/source/schema";
import { defineCollections, defineConfig, defineDocs } from "fumadocs-mdx/config";
import { z } from "zod";

export const docs = defineDocs({
  dir: "content/docs",
  docs: {
    postprocess: {
      includeProcessedMarkdown: true,
    },
  },
});

// Blog posts stay outside the docs collection: flat slugs, date/author
// frontmatter, and no entry in the docs sidebar/page tree.
export const blog = defineCollections({
  dir: "content/blog",
  type: "doc",
  schema: pageSchema.extend({
    date: z.iso.date(),
    author: z.string().optional(),
  }),
});

export default defineConfig({
  mdxOptions: {
    remarkPlugins: [remarkMdxMermaid],
  },
});
