import { createFileRoute, notFound } from "@tanstack/react-router";
import { TermsPage } from "@/components/terms-page";
import { i18n } from "@/lib/i18n";

export const Route = createFileRoute("/$lang/terms")({
  component: Page,
  beforeLoad: ({ params }) => {
    if (
      !i18n.languages.includes(params.lang as (typeof i18n.languages)[number])
    ) {
      throw notFound();
    }
  },
});

function Page() {
  const { lang } = Route.useParams();
  return <TermsPage lang={lang} />;
}
