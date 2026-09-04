import { Link } from "@tanstack/react-router";
import type { BaseLayoutProps } from "fumadocs-ui/layouts/shared";
import { i18n } from "./i18n";
import { appName, gitConfig } from "./shared";

export function baseOptions(lang: string = "en"): BaseLayoutProps {
  return {
    i18n,
    nav: {
      title: appName,
      // Point the navbar logo at the locale root instead of "/", which only
      // exists as a client-side redirect to /en. A raw <a href="/"> (fumadocs
      // renders a plain anchor, not a TanStack Link) re-hydrates the SPA shell
      // and runs the beforeLoad redirect, which hangs in the prerendered build.
      url: `/${lang}`,
    },
    githubUrl: `https://github.com/${gitConfig.user}/${gitConfig.repo}`,
    links: [
      {
        type: "icon",
        text: "X",
        icon: (
          <svg
            role="img"
            viewBox="0 0 24 24"
            className="size-4"
            fill="currentColor"
            aria-label="X"
          >
            <path d="M18.244 2.25h3.308l-7.227 8.26 8.502 11.24H16.17l-5.214-6.817L4.99 21.75H1.68l7.73-8.835L1.254 2.25H8.08l4.713 6.231zm-1.161 17.52h1.833L7.084 4.126H5.117z" />
          </svg>
        ),
        url: "https://x.com/timzaak",
      },
    ],
  };
}

export interface FooterLabels {
  copyright: string;
  privacy: string;
  terms: string;
}

// Single source for the footer's license/links copy so new pages don't
// re-inline the en/zh literals (and drift from the existing pages).
export function footerLabels(lang: string): FooterLabels {
  return lang === "zh"
    ? { copyright: "Apache 2.0", privacy: "隐私政策", terms: "服务条款" }
    : { copyright: "Apache 2.0", privacy: "Privacy", terms: "Terms" };
}

export function SiteFooter({
  lang,
  labels,
}: {
  lang: string;
  labels: FooterLabels;
}) {
  return (
    <footer className="relative z-10 border-t border-stone-200 dark:border-stone-800 py-10 px-4">
      <div className="max-w-5xl mx-auto flex flex-col sm:flex-row items-center justify-between gap-4 text-sm text-stone-500 dark:text-stone-400">
        <span>
          © {new Date().getFullYear()} {appName} · {labels.copyright}
        </span>
        <div className="flex items-center gap-6">
          <a
            href={`https://github.com/${gitConfig.user}/${gitConfig.repo}`}
            target="_blank"
            rel="noopener noreferrer"
            className="hover:text-amber-700 dark:hover:text-amber-400 transition-colors"
          >
            GitHub
          </a>
          <Link
            to="/blog"
            className="hover:text-amber-700 dark:hover:text-amber-400 transition-colors"
          >
            Blog
          </Link>
          <Link
            to="/$lang/privacy"
            params={{ lang }}
            className="hover:text-amber-700 dark:hover:text-amber-400 transition-colors"
          >
            {labels.privacy}
          </Link>
          <Link
            to="/$lang/terms"
            params={{ lang }}
            className="hover:text-amber-700 dark:hover:text-amber-400 transition-colors"
          >
            {labels.terms}
          </Link>
        </div>
      </div>
    </footer>
  );
}
