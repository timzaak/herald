import { createFileRoute, redirect } from "@tanstack/react-router";

// Blog is single-language at /blog — locale-prefixed URLs (language switcher,
// stale links) collapse onto it.
export const Route = createFileRoute("/$lang/blog/$")({
  beforeLoad: ({ params }) => {
    const slugs = params._splat?.split("/").filter(Boolean) ?? [];
    throw redirect({ href: `/${["blog", ...slugs].join("/")}` });
  },
});
