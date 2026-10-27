import { createFileRoute, notFound } from "@tanstack/react-router";
import { PrivacyPage } from "@/components/privacy-page";
import { i18n } from "@/lib/i18n";
import { legalDescriptions, seoHead } from "@/lib/seo";

export const Route = createFileRoute("/$lang/privacy")({
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
        params.lang === "zh" ? "隐私政策 — Herald" : "Privacy Policy — Herald",
      description: legalDescriptions[params.lang].privacy,
      path: `/${params.lang}/privacy`,
      locale: params.lang,
    }),
});

function Page() {
  const { lang } = Route.useParams();
  return <PrivacyPage lang={lang} />;
}
