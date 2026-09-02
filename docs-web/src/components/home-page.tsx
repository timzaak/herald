import { HomeLayout } from "fumadocs-ui/layouts/home";
import {
  Bot,
  ChevronDown,
  CreditCard,
  Plug,
  Rocket,
  ScrollText,
  Shield,
  Sparkles,
  Users,
} from "lucide-react";
import { Link } from "@tanstack/react-router";
import type { HomeTexts } from "@/lib/home-texts";
import { baseOptions, SiteFooter } from "@/lib/layout.shared";
import { gitConfig } from "@/lib/shared";

const GITHUB_URL = `https://github.com/${gitConfig.user}/${gitConfig.repo}`;
const DEMO_URL = "https://auth.fornetcode.com";

// Order must match the features array in home-texts.ts
const FEATURE_ICONS = [Shield, Users, CreditCard, ScrollText, Bot];

function GitHubIcon({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      className={className}
      fill="currentColor"
      aria-label="GitHub"
    >
      <path d="M12 2C6.477 2 2 6.484 2 12.017c0 4.425 2.865 8.18 6.839 9.504.5.092.682-.217.682-.483 0-.237-.008-.868-.013-1.703-2.782.605-3.369-1.343-3.369-1.343-.454-1.158-1.11-1.466-1.11-1.466-.908-.62.069-.608.069-.608 1.003.07 1.531 1.032 1.531 1.032.892 1.53 2.341 1.088 2.91.832.092-.647.35-1.088.636-1.338-2.22-.253-4.555-1.113-4.555-4.951 0-1.093.39-1.988 1.029-2.688-.103-.253-.446-1.272.098-2.65 0 0 .84-.27 2.75 1.026A9.564 9.564 0 0112 6.844c.85.004 1.705.115 2.504.337 1.909-1.296 2.747-1.027 2.747-1.027.546 1.379.202 2.398.1 2.651.64.7 1.028 1.595 1.028 2.688 0 3.848-2.339 4.695-4.566 4.943.359.309.678.92.678 1.855 0 1.338-.012 2.419-.012 2.747 0 .268.18.58.688.482A10.019 10.019 0 0022 12.017C22 6.484 17.522 2 12 2z" />
    </svg>
  );
}

export function HomePage({
  texts,
  docsLink,
}: {
  texts: HomeTexts;
  docsLink: { to: string; params: Record<string, string> };
}) {
  return (
    <HomeLayout {...baseOptions(docsLink.params.lang)}>
      <div className="relative overflow-hidden selection:bg-amber-200 selection:text-amber-900 dark:selection:bg-amber-800 dark:selection:text-amber-100">
        {/* Dot grid background */}
        <div
          className="absolute inset-0 pointer-events-none opacity-[0.035] dark:opacity-[0.06]"
          style={{
            backgroundImage:
              "radial-gradient(circle, currentColor 1px, transparent 1px)",
            backgroundSize: "24px 24px",
          }}
        />

        {/* Warm gradient glow */}
        <div className="absolute top-0 left-1/2 -translate-x-1/2 w-[900px] h-[600px] bg-amber-100/50 dark:bg-amber-900/10 rounded-full blur-[140px] pointer-events-none" />

        {/* Hero Section */}
        <section className="relative z-10 pt-28 pb-16 px-4">
          <div className="max-w-3xl mx-auto text-center">
            {/* Badge */}
            <div
              className="inline-flex items-center gap-2.5 bg-amber-50 dark:bg-amber-950/40 border border-amber-200/70 dark:border-amber-800/40 text-amber-800 dark:text-amber-300 px-4 py-1.5 rounded-full text-sm font-medium mb-8"
              style={{
                animation: "fade-up 0.6s ease-out both",
                animationDelay: "0ms",
              }}
            >
              <span className="w-1.5 h-1.5 bg-amber-500 rounded-full animate-pulse" />
              {texts.badge}
            </div>

            <h1
              className="text-4xl md:text-5xl lg:text-6xl font-serif font-bold text-stone-900 dark:text-stone-100 leading-[1.1] mb-6 tracking-tight"
              style={{
                animation: "fade-up 0.6s ease-out both",
                animationDelay: "80ms",
              }}
            >
              {texts.heroTitle}
            </h1>

            <p
              className="text-lg md:text-xl text-stone-500 dark:text-stone-400 mb-10 max-w-2xl mx-auto leading-relaxed"
              style={{
                animation: "fade-up 0.6s ease-out both",
                animationDelay: "160ms",
              }}
            >
              {texts.heroDesc}
            </p>

            <div
              className="flex flex-col sm:flex-row items-center justify-center gap-3 flex-wrap"
              style={{
                animation: "fade-up 0.6s ease-out both",
                animationDelay: "240ms",
              }}
            >
              <a
                href={GITHUB_URL}
                target="_blank"
                rel="noopener noreferrer"
                className="flex items-center gap-2 bg-amber-600 hover:bg-amber-700 text-white px-6 py-3 rounded-full font-medium transition-all duration-200 shadow-lg shadow-amber-600/20 dark:shadow-amber-900/40 justify-center"
              >
                <GitHubIcon className="w-5 h-5" />
                {texts.starGithub}
              </a>
              <Link
                {...docsLink}
                className="flex items-center gap-2 bg-white/70 dark:bg-stone-800/70 hover:bg-white dark:hover:bg-stone-800 backdrop-blur-md text-stone-700 dark:text-stone-300 px-6 py-3 rounded-full border border-stone-200 dark:border-stone-700 font-medium transition-all duration-200 shadow-sm justify-center"
              >
                {texts.getStarted}
              </Link>
              <a
                href={DEMO_URL}
                target="_blank"
                rel="noopener noreferrer"
                className="flex items-center gap-2 text-stone-600 dark:text-stone-400 hover:text-amber-700 dark:hover:text-amber-400 px-6 py-3 rounded-full font-medium transition-colors justify-center"
              >
                {texts.liveDemo}
              </a>
            </div>
          </div>

          {/* Terminal Block */}
          <div
            className="relative mt-20 max-w-2xl mx-auto"
            style={{
              animation: "fade-up 0.7s ease-out both",
              animationDelay: "350ms",
            }}
          >
            <div className="absolute inset-0 bg-gradient-to-r from-amber-300/20 via-orange-200/20 to-amber-300/20 blur-2xl rounded-3xl transform scale-y-75 translate-y-4" />

            <div className="relative bg-stone-900 dark:bg-stone-950 border border-stone-800 dark:border-stone-800 shadow-[0_24px_60px_rgba(0,0,0,0.15)] rounded-2xl overflow-hidden z-10">
              <div className="flex items-center gap-2 px-4 py-3 border-b border-stone-800/60">
                <div className="w-3 h-3 rounded-full bg-red-400/70" />
                <div className="w-3 h-3 rounded-full bg-yellow-400/70" />
                <div className="w-3 h-3 rounded-full bg-green-400/70" />
                <span className="ml-3 text-stone-500 text-xs font-mono">
                  {texts.terminal.label}
                </span>
              </div>
              <div className="px-5 py-5 md:py-6 bg-stone-900/50 font-mono text-sm md:text-base overflow-x-auto space-y-1">
                {texts.terminal.lines.map((line) => (
                  <div key={line.text} className="whitespace-nowrap">
                    {line.prefix && (
                      <span className="text-amber-400 select-none mr-2 font-semibold">
                        {line.prefix}
                      </span>
                    )}
                    <span className={lineColor(line.status)}>{line.text}</span>
                  </div>
                ))}
                <span className="inline-block w-[7px] h-[18px] bg-amber-400/90 mt-1 animate-pulse rounded-[1px]" />
              </div>
            </div>
          </div>
        </section>

        <Divider />

        {/* Features Section */}
        <section
          id="features"
          className="relative z-10 py-20 max-w-5xl mx-auto px-4 scroll-mt-24"
        >
          <div className="text-center mb-14">
            <h2 className="text-3xl md:text-4xl font-serif font-bold text-stone-900 dark:text-stone-100 tracking-tight">
              {texts.featureSectionTitle}
            </h2>
            <p className="mt-3 text-stone-500 dark:text-stone-400 max-w-2xl mx-auto">
              {texts.featureSectionDesc}
            </p>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-3 gap-6 cursor-default">
            {texts.features.map((feature, i) => {
              const Icon = FEATURE_ICONS[i];
              return (
                <FeatureModule
                  key={feature.title}
                  icon={<Icon className="w-6 h-6" />}
                  title={feature.title}
                  desc={feature.desc}
                  bullets={feature.bullets}
                />
              );
            })}
          </div>
        </section>

        <Divider />

        {/* How it works */}
        <section className="relative z-10 py-20 px-4">
          <div className="max-w-xl mx-auto">
            <div className="text-center mb-16">
              <h2 className="text-3xl md:text-4xl font-serif font-bold text-stone-900 dark:text-stone-100 tracking-tight">
                {texts.stepsSectionTitle}
              </h2>
              <p className="mt-3 text-stone-500 dark:text-stone-400">
                {texts.stepsSectionDesc}
              </p>
            </div>

            <div className="relative space-y-10">
              <div className="absolute top-7 left-[1.75rem] bottom-7 w-px bg-stone-200 dark:bg-stone-700 hidden sm:block pointer-events-none" />

              <Step
                icon={<Rocket className="w-7 h-7" strokeWidth={1.5} />}
                num={texts.steps[0].num}
                title={texts.steps[0].title}
                desc={texts.steps[0].desc}
              />
              <Step
                icon={<Sparkles className="w-7 h-7" strokeWidth={1.5} />}
                num={texts.steps[1].num}
                title={texts.steps[1].title}
                desc={texts.steps[1].desc}
              />
              <Step
                icon={<Plug className="w-7 h-7" strokeWidth={1.5} />}
                num={texts.steps[2].num}
                title={texts.steps[2].title}
                desc={texts.steps[2].desc}
              />
            </div>
          </div>
        </section>

        <Divider />

        {/* Comparison */}
        <section className="relative z-10 py-20 px-4">
          <div className="max-w-4xl mx-auto">
            <div className="text-center mb-14">
              <h2 className="text-3xl md:text-4xl font-serif font-bold text-stone-900 dark:text-stone-100 tracking-tight">
                {texts.compareSectionTitle}
              </h2>
              <p className="mt-3 text-stone-500 dark:text-stone-400">
                {texts.compareSectionDesc}
              </p>
            </div>

            <div className="bg-white dark:bg-stone-900 rounded-2xl shadow-[0_8px_40px_rgb(0,0,0,0.04)] dark:shadow-[0_8px_40px_rgb(0,0,0,0.3)] border border-stone-100 dark:border-stone-800 overflow-hidden">
              <div className="overflow-x-auto">
                <table className="w-full text-left border-collapse min-w-[600px]">
                  <thead>
                    <tr className="border-b border-stone-100 dark:border-stone-800">
                      <th className="px-6 py-5 font-medium text-stone-400 dark:text-stone-500 w-1/4" />
                      <th className="px-6 py-5 w-1/4">
                        <span className="text-lg font-bold text-amber-700 dark:text-amber-400">
                          {texts.compareHeaders.herald}
                        </span>
                      </th>
                      <th className="px-6 py-5 font-bold text-stone-500 dark:text-stone-500 w-1/4 text-lg">
                        {texts.compareHeaders.auth0}
                      </th>
                      <th className="px-6 py-5 font-bold text-stone-500 dark:text-stone-500 w-1/4 text-lg">
                        {texts.compareHeaders.supabase}
                      </th>
                      <th className="px-6 py-5 font-bold text-stone-500 dark:text-stone-500 w-1/4 text-lg">
                        {texts.compareHeaders.keycloak}
                      </th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-stone-50 dark:divide-stone-800/60 text-stone-600 dark:text-stone-400">
                    {texts.compareRows.map((row) => (
                      <CompareRow key={row.label} {...row} />
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          </div>
        </section>

        <Divider />

        {/* FAQ */}
        <section className="relative z-10 py-20 px-4">
          <div className="max-w-3xl mx-auto">
            <div className="text-center mb-14">
              <h2 className="text-3xl md:text-4xl font-serif font-bold text-stone-900 dark:text-stone-100 tracking-tight">
                {texts.faqSectionTitle}
              </h2>
              <p className="mt-3 text-stone-500 dark:text-stone-400">
                {texts.faqSectionDesc}
              </p>
            </div>

            <div className="space-y-3">
              {texts.faq.map((item) => (
                <FAQItem
                  key={item.question}
                  question={item.question}
                  answer={item.answer}
                />
              ))}
            </div>
          </div>
        </section>

        <Divider />

        {/* Bottom CTA */}
        <section className="relative z-10 py-20 px-4 text-center">
          <h2 className="text-3xl md:text-4xl font-serif font-bold text-stone-900 dark:text-stone-100 mb-3 tracking-tight max-w-3xl mx-auto">
            {texts.ctaTitle}
          </h2>
          <p className="text-xl text-stone-500 dark:text-stone-400 mb-10 max-w-2xl mx-auto">
            {texts.ctaDesc}
          </p>

          <div className="flex flex-col sm:flex-row items-center justify-center gap-4 flex-wrap">
            <a
              href={GITHUB_URL}
              target="_blank"
              rel="noopener noreferrer"
              className="flex items-center gap-2 bg-amber-600 hover:bg-amber-700 text-white px-8 py-3.5 rounded-full font-medium transition-all duration-200 shadow-lg shadow-amber-600/20 dark:shadow-amber-900/40 justify-center"
            >
              <GitHubIcon className="w-5 h-5" />
              {texts.starGithub}
            </a>
            <Link
              {...docsLink}
              className="flex items-center gap-2 bg-white/70 dark:bg-stone-800/70 hover:bg-white dark:hover:bg-stone-800 backdrop-blur-md text-stone-700 dark:text-stone-300 px-8 py-3.5 rounded-full border border-stone-200 dark:border-stone-700 font-medium transition-all duration-200 shadow-sm justify-center"
            >
              {texts.getStarted}
            </Link>
          </div>
        </section>

        <SiteFooter lang={docsLink.params.lang} labels={texts.footer} />
      </div>
    </HomeLayout>
  );
}

function lineColor(status?: "command" | "ok" | "info") {
  switch (status) {
    case "command":
      return "text-stone-200";
    case "ok":
      return "text-green-400";
    case "info":
      return "text-stone-400";
    default:
      return "text-stone-300";
  }
}

function Divider() {
  return (
    <div className="flex items-center justify-center py-1 max-w-4xl mx-auto px-4">
      <div className="h-px flex-1 max-w-12 bg-stone-200 dark:bg-stone-800" />
      <div className="mx-3 w-1.5 h-1.5 bg-amber-400/60 dark:bg-amber-600/40 rotate-45" />
      <div className="h-px flex-1 max-w-12 bg-stone-200 dark:bg-stone-800" />
    </div>
  );
}

function FeatureModule({
  icon,
  title,
  desc,
  bullets,
}: {
  icon: React.ReactNode;
  title: string;
  desc: string;
  bullets: string[];
}) {
  return (
    <div className="group bg-white dark:bg-stone-900 rounded-2xl py-7 px-6 flex flex-col items-start gap-4 border border-stone-100 dark:border-stone-800 hover:border-amber-200/80 dark:hover:border-amber-800/50 hover:shadow-lg hover:shadow-amber-50/50 dark:hover:shadow-none transition-all duration-300">
      <div className="text-amber-600 dark:text-amber-400 p-2.5 bg-amber-50/80 dark:bg-amber-900/20 rounded-lg">
        {icon}
      </div>
      <h3 className="font-semibold text-stone-900 dark:text-stone-100 tracking-tight text-lg">
        {title}
      </h3>
      <p className="text-sm text-stone-500 dark:text-stone-400 leading-relaxed">
        {desc}
      </p>
      <ul className="space-y-2 mt-1">
        {bullets.map((b) => (
          <li
            key={b}
            className="flex items-start gap-2 text-sm text-stone-600 dark:text-stone-400"
          >
            <span className="w-1.5 h-1.5 rounded-full bg-amber-500 shrink-0 mt-1.5" />
            <span>{b}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

function Step({
  icon,
  num,
  title,
  desc,
}: {
  icon: React.ReactNode;
  num: string;
  title: string;
  desc: string;
}) {
  return (
    <div className="flex flex-col sm:flex-row items-center sm:items-start gap-4 sm:gap-6 relative z-10 group">
      <div className="bg-white dark:bg-stone-900 border border-stone-100 dark:border-stone-800 p-3.5 rounded-xl shadow-sm group-hover:border-amber-200 dark:group-hover:border-amber-800/50 transition-colors text-amber-600 dark:text-amber-400 relative z-10">
        {icon}
      </div>

      <div className="flex-1 text-center sm:text-left mt-1 sm:mt-1.5">
        <div className="flex items-center gap-2.5 justify-center sm:justify-start mb-1.5">
          <span className="text-[11px] font-semibold text-amber-600 dark:text-amber-400 font-mono tracking-wider">
            {num}
          </span>
          <h3 className="text-lg font-bold text-stone-900 dark:text-stone-100 font-serif tracking-tight">
            {title}
          </h3>
        </div>
        <p className="text-stone-600 dark:text-stone-400 leading-relaxed text-sm max-w-sm mx-auto sm:mx-0">
          {desc}
        </p>
      </div>
    </div>
  );
}

function CompareRow({
  label,
  herald,
  auth0,
  supabase,
  keycloak,
}: {
  label: string;
  herald: string;
  auth0: string;
  supabase: string;
  keycloak: string;
}) {
  return (
    <tr className="hover:bg-amber-50/30 dark:hover:bg-amber-950/10 transition-colors">
      <td className="px-6 py-5 font-semibold text-stone-900 dark:text-stone-200 border-r border-stone-50 dark:border-stone-800/60">
        {label}
      </td>
      <td className="px-6 py-5 border-r border-stone-50 dark:border-stone-800/60">
        <div className="flex items-center gap-2.5">
          <span className="w-1.5 h-1.5 rounded-full bg-amber-500 shrink-0" />
          <span className="text-stone-800 dark:text-stone-200 font-medium">
            {herald}
          </span>
        </div>
      </td>
      <td className="px-6 py-5 border-r border-stone-50 dark:border-stone-800/60 text-stone-500 dark:text-stone-500">
        {auth0}
      </td>
      <td className="px-6 py-5 border-r border-stone-50 dark:border-stone-800/60 text-stone-500 dark:text-stone-500">
        {supabase}
      </td>
      <td className="px-6 py-5 text-stone-500 dark:text-stone-500">
        {keycloak}
      </td>
    </tr>
  );
}

function FAQItem({ question, answer }: { question: string; answer: string }) {
  return (
    <details className="group bg-white dark:bg-stone-900 rounded-xl border border-stone-100 dark:border-stone-800 open:border-amber-200/80 dark:open:border-amber-800/50 transition-colors">
      <summary className="flex items-center justify-between gap-4 px-5 py-4 cursor-pointer list-none select-none">
        <span className="font-semibold text-stone-900 dark:text-stone-100">
          {question}
        </span>
        <ChevronDown className="w-5 h-5 text-stone-400 group-open:rotate-180 transition-transform shrink-0" />
      </summary>
      <div className="px-5 pb-5 text-stone-600 dark:text-stone-400 leading-relaxed text-sm">
        {answer}
      </div>
    </details>
  );
}
