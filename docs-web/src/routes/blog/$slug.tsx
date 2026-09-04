import { createFileRoute, notFound } from "@tanstack/react-router";
import { createServerFn } from "@tanstack/react-start";
import { staticFunctionMiddleware } from "@tanstack/start-static-server-functions";
import browserCollections from "collections/browser";
import { useFumadocsLoader } from "fumadocs-core/source/client";
import { InlineTOC } from "fumadocs-ui/components/inline-toc";
import { HomeLayout } from "fumadocs-ui/layouts/home";
import { Suspense } from "react";
import { useMDXComponents } from "@/components/mdx";
import { formatPostDate } from "@/lib/blog";
import { baseOptions, footerLabels, SiteFooter } from "@/lib/layout.shared";
import { blogSource } from "@/lib/source";

export const Route = createFileRoute("/blog/$slug")({
  component: Page,
  loader: async ({ params }) => {
    const data = await loader({ data: { slug: params.slug } });
    await clientLoader.preload(data.path);
    return data;
  },
});

const loader = createServerFn({ method: "GET" })
  .inputValidator((input: { slug: string }) => input)
  .middleware([staticFunctionMiddleware])
  .handler(async ({ data: { slug } }) => {
    const page = blogSource.getPage([slug]);
    if (!page) throw notFound();
    return {
      path: page.path,
    };
  });

const clientLoader = browserCollections.blog.createClientLoader({
  component({ toc, frontmatter, default: MDX }) {
    return (
      <article className="max-w-3xl mx-auto prose">
        <h1 className="text-4xl md:text-5xl font-serif font-bold text-stone-900 dark:text-stone-100 mb-2 tracking-tight">
          {frontmatter.title}
        </h1>
        <p className="text-stone-500 dark:text-stone-400 text-sm mb-10">
          {formatPostDate(frontmatter.date)}
          {frontmatter.author ? ` · ${frontmatter.author}` : ""}
        </p>
        {toc.length > 0 ? <InlineTOC items={toc} /> : null}
        {/* biome-ignore lint/correctness/useHookAtTopLevel: fumadocs clientLoader component is a render function */}
        <MDX components={useMDXComponents()} />
      </article>
    );
  },
});

function Page() {
  const data = useFumadocsLoader(Route.useLoaderData());
  const content = clientLoader.useContent(data.path);

  return (
    <HomeLayout {...baseOptions()}>
      <div className="relative z-10 py-20 px-4 min-h-[60vh]">
        <Suspense>{content}</Suspense>
      </div>
      <SiteFooter lang="en" labels={footerLabels("en")} />
    </HomeLayout>
  );
}
