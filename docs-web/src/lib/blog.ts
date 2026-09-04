export interface BlogPostSummary {
  slug: string;
  title: string;
  description: string | undefined;
  date: string;
  author: string | undefined;
}

// ISO date strings ("2026-09-03") parse at UTC midnight; format in UTC so the
// displayed day never shifts in negative-offset timezones.
export function formatPostDate(date: string) {
  return new Date(date).toLocaleDateString("en-US", {
    year: "numeric",
    month: "long",
    day: "numeric",
    timeZone: "UTC",
  });
}
