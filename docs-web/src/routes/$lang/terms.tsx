import { createFileRoute, notFound } from "@tanstack/react-router";
import { TermsPage } from "@/components/terms-page";
import { i18n } from "@/lib/i18n";
import { legalDescriptions, seoHead } from "@/lib/seo";

export const Route = createFileRoute("/$lang/terms")({
  component: Page,
  beforeLoad: ({ params }) => {
    if (
      !i18n.languages.includes(params.lang as (typeof i18n.languages)[number])
    ) {
      throw notFound();
    }
  },
  head: ({ params }) =>
    seoHead({
      title:
        params.lang === "zh"
          ? "服务条款 — Herald"
          : "Terms of Service — Herald",
      description: legalDescriptions[params.lang].terms,
      path: `/${params.lang}/terms`,
      locale: params.lang,
    }),
});

function Page() {
  const { lang } = Route.useParams();
  return <TermsPage lang={lang} />;
}
