import { Link } from "@tanstack/react-router";
import { HomeLayout } from "fumadocs-ui/layouts/home";
import { type BlogPostSummary, formatPostDate } from "@/lib/blog";
import { baseOptions, footerLabels, SiteFooter } from "@/lib/layout.shared";

export function BlogList({ posts }: { posts: BlogPostSummary[] }) {
  return (
    <HomeLayout {...baseOptions()}>
      <div className="relative z-10 py-20 px-4 min-h-[60vh]">
        <div className="max-w-3xl mx-auto">
          <h1 className="text-4xl md:text-5xl font-serif font-bold text-stone-900 dark:text-stone-100 mb-2 tracking-tight">
            Blog
          </h1>
          <p className="text-stone-500 dark:text-stone-400 text-sm mb-12">
            Release notes, engineering deep dives, and roadmap updates.
          </p>

          <div className="grid gap-4">
            {posts.map((post) => (
              <Link
                key={post.slug}
                to="/blog/$slug"
                params={{ slug: post.slug }}
                className="group block rounded-xl border border-stone-200 dark:border-stone-800 p-6 transition-colors hover:border-amber-600/60 dark:hover:border-amber-400/40"
              >
                <h2 className="text-xl font-bold text-stone-900 dark:text-stone-100 transition-colors group-hover:text-amber-700 dark:group-hover:text-amber-400">
                  {post.title}
                </h2>
                {post.description ? (
                  <p className="text-stone-600 dark:text-stone-400 mt-2 leading-relaxed">
                    {post.description}
                  </p>
                ) : null}
                <p className="text-sm text-stone-500 dark:text-stone-500 mt-4">
                  {formatPostDate(post.date)}
                  {post.author ? ` · ${post.author}` : ""}
                </p>
              </Link>
            ))}
          </div>
        </div>
      </div>

      <SiteFooter lang="en" labels={footerLabels("en")} />
    </HomeLayout>
  );
}
