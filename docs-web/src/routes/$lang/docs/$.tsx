import {
  createFileRoute,
  Link,
  notFound,
  redirect,
} from "@tanstack/react-router";
import { createServerFn } from "@tanstack/react-start";
import { staticFunctionMiddleware } from "@tanstack/start-static-server-functions";
import browserCollections from "collections/browser";
import { useFumadocsLoader } from "fumadocs-core/source/client";
import { DocsLayout } from "fumadocs-ui/layouts/docs";
import {
  DocsBody,
  DocsDescription,
  DocsPage,
  DocsTitle,
  MarkdownCopyButton,
  ViewOptionsPopover,
} from "fumadocs-ui/layouts/docs/page";
import { Suspense } from "react";
import { ClientAPIPage } from "@/components/api-page";
import { useMDXComponents } from "@/components/mdx";
import { i18n } from "@/lib/i18n";
import {
  baseOptions,
  footerLabels,
  SiteFooter,
} from "@/lib/layout.shared";
import { gitConfig } from "@/lib/shared";
import { slugsToMarkdownPath, source } from "@/lib/source";

export const Route = createFileRoute("/$lang/docs/$")({
  component: Page,
  loader: async ({ params }) => {
    const locale = params.lang;
    if (!i18n.languages.includes(locale as (typeof i18n.languages)[number])) {
      throw notFound();
    }
    const slugs = params._splat?.split("/").filter(Boolean) ?? [];
    // /zh/docs → redirect to getting-started
    if (slugs.length === 0) {
      throw redirect({ href: `/${locale}/docs/getting-started` });
    }
    const data = await loader({ data: { locale, slugs } });
    // Fumadocs MDX: only preload content for normal pages
    if (data.type === "docs") {
      await clientLoader.preload(data.path);
    }
    return data;
  },
});

const loader = createServerFn({
  method: "GET",
})
  .inputValidator((input: { locale: string; slugs: string[] }) => input)
  .middleware([staticFunctionMiddleware])
  .handler(async ({ data: { locale, slugs } }) => {
    // `/docs/openapi` has no index page (fumadocs-openapi generates one page
    // per operation under `<tag>/<operationId>`). MDX links point here as the
    // "OpenAPI reference" entry point, so redirect to the first real page.
    // Uses the loader's emitted order, so it stays valid as the spec changes.
    if (slugs.length === 1 && slugs[0] === "openapi") {
      const first = source.getPages(locale).find((p) => p.data.type === "openapi");
      if (first?.url) throw redirect({ href: first.url });
    }

    const page = source.getPage(slugs, locale);
    if (!page) throw notFound();

    const pageTree = await source.serializePageTree(source.getPageTree(locale));

    if (page.data.type === "openapi") {
      return {
        type: "openapi" as const,
        title: page.data.title,
        description: page.data.description,
        locale,
        pageTree,
        props: await page.data.getClientAPIPageProps(),
      };
    }

    return {
      type: "docs" as const,
      path: page.path,
      locale,
      markdownUrl: slugsToMarkdownPath(page.slugs, locale).url,
      pageTree,
    };
  });

const clientLoader = browserCollections.docs.createClientLoader({
  component(
    { toc, frontmatter, default: MDX },
    {
      markdownUrl,
      path,
    }: {
      markdownUrl: string;
      path: string;
    },
  ) {
    return (
      <DocsPage toc={toc} footer={{ enabled: false }}>
        <DocsTitle>{frontmatter.title}</DocsTitle>
        <DocsDescription>{frontmatter.description}</DocsDescription>
        <div className="flex flex-row gap-2 items-center border-b -mt-4 pb-6">
          <MarkdownCopyButton markdownUrl={markdownUrl} />
          <ViewOptionsPopover
            markdownUrl={markdownUrl}
            githubUrl={`https://github.com/${gitConfig.user}/${gitConfig.repo}/blob/${gitConfig.branch}/content/docs/${path}`}
          />
        </div>
        <DocsBody>
          {/* biome-ignore lint/correctness/useHookAtTopLevel: fumadocs clientLoader component is a render function */}
          <MDX components={useMDXComponents()} />
        </DocsBody>
      </DocsPage>
    );
  },
});

function Page() {
  const data = useFumadocsLoader(Route.useLoaderData());

  let content: React.ReactNode;
  if (data.type === "openapi") {
    content = (
      <DocsPage full footer={{ enabled: false }}>
        <DocsTitle>{data.title}</DocsTitle>
        <DocsDescription>{data.description}</DocsDescription>
        <DocsBody>
          <ClientAPIPage {...data.props} />
        </DocsBody>
      </DocsPage>
    );
  } else {
    // biome-ignore lint/correctness/useHookAtTopLevel: fumadocs framework pattern
    content = clientLoader.useContent(data.path, {
      markdownUrl: data.markdownUrl,
      path: data.path,
    });
  }

  return (
    <DocsLayout {...baseOptions(data.locale)} tree={data.pageTree}>
      {data.type === "docs" && <Link to={data.markdownUrl} hidden />}
      <Suspense>{content}</Suspense>
      <SiteFooter lang={data.locale} labels={footerLabels(data.locale)} />
    </DocsLayout>
  );
}
