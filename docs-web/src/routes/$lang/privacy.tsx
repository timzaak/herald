import { createFileRoute, notFound } from "@tanstack/react-router";
import { PrivacyPage } from "@/components/privacy-page";
import { i18n } from "@/lib/i18n";

export const Route = createFileRoute("/$lang/privacy")({
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
  return <PrivacyPage lang={lang} />;
}
