import { createFileRoute } from "@tanstack/react-router";
import { createServerFn } from "@tanstack/react-start";
import { staticFunctionMiddleware } from "@tanstack/start-static-server-functions";
import { useFumadocsLoader } from "fumadocs-core/source/client";
import { BlogList } from "@/components/blog-list";
import type { BlogPostSummary } from "@/lib/blog";
import { blogDescription, seoHead } from "@/lib/seo";
import { blogSource } from "@/lib/source";

export const Route = createFileRoute("/blog/")({
  component: Page,
  loader: () => loader(),
  head: () =>
    seoHead({
      title: "Blog — Herald",
      description: blogDescription,
      path: "/blog",
    }),
});

const loader = createServerFn({ method: "GET" })
  .middleware([staticFunctionMiddleware])
  .handler(async () => {
    const posts: BlogPostSummary[] = blogSource
      .getPages()
      .map((page) => ({
        slug: page.slugs[0],
        title: page.data.title,
        description: page.data.description,
        date: page.data.date,
        author: page.data.author,
      }))
      .sort((a, b) => b.date.localeCompare(a.date));
    return { posts };
  });

function Page() {
  const data = useFumadocsLoader(Route.useLoaderData());
  return <BlogList posts={data.posts} />;
}
