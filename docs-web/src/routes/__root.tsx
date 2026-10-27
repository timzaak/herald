import {
  createRootRoute,
  HeadContent,
  Outlet,
  Scripts,
  useLocation,
} from "@tanstack/react-router";
import { defineI18nUI } from "fumadocs-ui/i18n";
import { RootProvider } from "fumadocs-ui/provider/tanstack";
import RWikiChatWidget from "@/components/rwiki-chat-widget";
import SearchDialog from "@/components/search";
import { i18n } from "@/lib/i18n";
import { homeTitles, seoMeta, siteDescriptions } from "@/lib/seo";
import appCss from "@/styles/app.css?url";

const GA_ID = "G-G6EGB7SXZV";

const { provider } = defineI18nUI(i18n, {
  en: { displayName: "English", search: "Search Docs" },
  zh: { displayName: "中文", search: "搜索文档" },
});

function useLocale() {
  const location = useLocation();
  const segments = location.pathname.split("/").filter(Boolean);
  if (
    segments.length > 0 &&
    i18n.languages.includes(segments[0] as (typeof i18n.languages)[number])
  ) {
    return segments[0];
  }
  return i18n.defaultLanguage;
}

export const Route = createRootRoute({
  head: () => ({
    // Site-wide defaults (also the only meta the "/" SPA shell ever gets).
    // Leaf routes override every key via seoMeta(); TanStack Router keeps the
    // deepest match per name/property. No canonical here — links are appended
    // across matches, so only leaf routes may emit one.
    meta: [
      {
        charSet: "utf-8",
      },
      {
        name: "viewport",
        content: "width=device-width, initial-scale=1",
      },
      ...seoMeta({
        title: homeTitles.en,
        description: siteDescriptions.en,
        path: "/",
      }),
      // Google Search Console verification — replace content with your actual token
      // {
      //   name: "google-site-verification",
      //   content: "YOUR_VERIFICATION_TOKEN",
      // },
    ],
    links: [
      { rel: "stylesheet", href: appCss },
      { rel: "icon", href: "/icon.svg", type: "image/svg+xml" },
      { rel: "icon", href: "/favicon.ico", sizes: "any" },
      { rel: "apple-touch-icon", href: "/apple-touch-icon.png" },
    ],
    scripts: [
      {
        src: `https://www.googletagmanager.com/gtag/js?id=${GA_ID}`,
        async: true,
      },
      {
        type: "text/javascript",
        children: `window.dataLayer = window.dataLayer || [];
function gtag(){dataLayer.push(arguments);}
gtag('js', new Date());
gtag('config', '${GA_ID}');`,
      },
    ],
  }),
  component: RootComponent,
});

function RootComponent() {
  const locale = useLocale();
  return (
    <html lang={locale} suppressHydrationWarning>
      <head>
        <HeadContent />
      </head>
      <body className="flex flex-col min-h-screen">
        <RootProvider search={{ SearchDialog }} i18n={provider(locale)}>
          <Outlet />
        </RootProvider>
        <Scripts />
        <RWikiChatWidget locale={locale} />
      </body>
    </html>
  );
}
