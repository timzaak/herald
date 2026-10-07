import { createFileRoute, notFound, Outlet } from "@tanstack/react-router";
import { i18n } from "@/lib/i18n";

export const Route = createFileRoute("/$lang")({
  component: () => <Outlet />,
  loader: ({ params }) => {
    if (
      !i18n.languages.includes(params.lang as (typeof i18n.languages)[number])
    ) {
      throw notFound();
    }
  },
});
