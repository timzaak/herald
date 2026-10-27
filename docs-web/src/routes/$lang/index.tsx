import { createFileRoute } from "@tanstack/react-router";
import { HomePage } from "@/components/home-page";
import { en, type HomeTexts, zh } from "@/lib/home-texts";
import { homeJsonLd, homeTitles, seoHead, siteDescriptions } from "@/lib/seo";

const textsMap: Record<string, HomeTexts> = { en, zh };

export const Route = createFileRoute("/$lang/")({
  component: LangHome,
  head: ({ params }) => ({
    ...seoHead({
      title: homeTitles[params.lang],
      description: siteDescriptions[params.lang],
      path: `/${params.lang}`,
      locale: params.lang,
    }),
    scripts: [
      {
        type: "application/ld+json",
        children: JSON.stringify(homeJsonLd(params.lang)),
      },
    ],
  }),
});

function LangHome() {
  const { lang } = Route.useParams();
  const texts = textsMap[lang] ?? en;

  return (
    <HomePage
      texts={texts}
      docsLink={{
        to: "/$lang/docs/$",
        params: { lang, _splat: "getting-started" },
      }}
    />
  );
}
