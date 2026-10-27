// Single source for the SEO head signals — title/description/og/canonical.
//
// Most directory-site crawlers, search engines, and AI assistants read only
// the prerendered <head>; they never run the SPA. These tags are the site's
// only reliable identity signal, so the copy here must stay in sync with the
// approved submission copy (market repo docs/promotion-submissions.md).

import { appName } from "./shared";

export const siteUrl = "https://www.fornetcode.com";
export const siteOgImage = `${siteUrl}/og-image.png`;

export const homeTitles: Record<string, string> = {
  en: "Herald — Open-source Self-hosted Auth & Billing for AI Apps",
  zh: "Herald — AI 产品的开源认证与计费后端",
};

export const siteDescriptions: Record<string, string> = {
  en: "Open-source (Apache-2.0) auth + billing platform for AI products: multi-tenant identity (passkeys, OIDC, API keys), Stripe/Creem/WeChat Pay, prepaid credits and usage quotas. Single Docker process, no per-MAU fees, data stays on your server.",
  zh: "开源（Apache-2.0）的 AI 产品认证与计费平台：多租户身份（Passkey、OIDC、API Key）、Stripe/Creem/微信支付、预付积分与用量配额。单个 Docker 进程，无按 MAU 收费，数据留在你自己的服务器。",
};

export const blogDescription =
  "Release notes, engineering deep dives, and roadmap updates for Herald.";

export const legalDescriptions: Record<
  string,
  Record<"privacy" | "terms", string>
> = {
  en: {
    privacy: "Privacy policy for the Herald website and demo service.",
    terms: "Terms of service for the Herald website and demo service.",
  },
  zh: {
    privacy: "Herald 网站与演示服务的隐私政策。",
    terms: "Herald 网站与演示服务的服务条款。",
  },
};

interface SeoMetaInput {
  title: string;
  description: string;
  /** Canonical path including locale prefix, e.g. "/en/docs/getting-started". */
  path: string;
  /** Site locale ("en" | "zh"); omit on locale-neutral pages (e.g. /blog). */
  locale?: string;
}

type MetaEntry =
  | { title: string }
  | { name: string; content: string }
  | { property: string; content: string };

// Root route repeats the same name/property keys with default copy; TanStack
// Router dedupes head metas by name/property with the deepest match winning,
// so leaf values here always override the root defaults.
export function seoMeta({
  title,
  description,
  path,
  locale,
}: SeoMetaInput): MetaEntry[] {
  const metas: MetaEntry[] = [
    { title },
    { name: "description", content: description },
    { property: "og:title", content: title },
    { property: "og:description", content: description },
    { property: "og:url", content: `${siteUrl}${path}` },
    { property: "og:image", content: siteOgImage },
    { property: "og:site_name", content: appName },
    { property: "og:type", content: "website" },
    { name: "twitter:card", content: "summary_large_image" },
    { name: "twitter:title", content: title },
    { name: "twitter:description", content: description },
    { name: "twitter:image", content: siteOgImage },
  ];
  if (locale) {
    metas.push({
      property: "og:locale",
      content: locale === "zh" ? "zh_CN" : "en_US",
    });
  }
  return metas;
}

// Leaf-route head helper: one input produces the meta plus the matching
// canonical link, so og:url and the canonical href cannot drift apart.
// Canonical links are appended (not deduped) across matched routes, so only
// leaf routes may call this — never the root defaults.
export function seoHead(input: SeoMetaInput) {
  return {
    meta: seoMeta(input),
    links: [{ rel: "canonical", href: `${siteUrl}${input.path}` }],
  };
}

// Rendered via TanStack's `script:ld+json` meta form on the home pages.
export function homeJsonLd(locale: string) {
  return {
    "@context": "https://schema.org",
    "@type": "SoftwareApplication",
    name: appName,
    applicationCategory: "DeveloperApplication",
    operatingSystem: "Linux, Docker",
    url: siteUrl,
    image: siteOgImage,
    description: siteDescriptions[locale] ?? siteDescriptions.en,
    license: "https://www.apache.org/licenses/LICENSE-2.0",
    offers: { "@type": "Offer", price: "0", priceCurrency: "USD" },
  };
}
