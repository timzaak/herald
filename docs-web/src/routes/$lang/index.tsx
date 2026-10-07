import { createFileRoute } from "@tanstack/react-router";
import { HomePage } from "@/components/home-page";
import { en, type HomeTexts, zh } from "@/lib/home-texts";

const textsMap: Record<string, HomeTexts> = { en, zh };

export const Route = createFileRoute("/$lang/")({
  component: LangHome,
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
