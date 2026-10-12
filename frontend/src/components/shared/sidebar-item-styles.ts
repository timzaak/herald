/**
 * Shared nav-item styling for the /manage admin sidebar and the /user profile
 * sidebar so the two navigation rails read as one component family: the item
 * wrapper (rounded hover, 150ms transitions, active-state indicator bar) and
 * the menu icon sizing/opacity behavior live here instead of being mirrored
 * across the two sidebar files.
 */

export const SIDEBAR_ITEM_ACTIVE_CLASS =
  'bg-sidebar-accent text-sidebar-foreground font-semibold before:absolute before:left-0 before:top-1/2 before:-translate-y-1/2 before:h-5 before:w-[3px] before:rounded-r-full before:bg-sidebar-primary'

export const SIDEBAR_ITEM_ICON_CLASS =
  'size-[18px] shrink-0 opacity-60 group-hover:opacity-100 group-[.font-semibold]:opacity-100 transition-opacity'

export function sidebarItemClassName(active: boolean, padding = 'px-4'): string {
  return `group relative flex items-center gap-3 rounded-lg ${padding} py-2.5 text-sm font-medium transition-all duration-150 ${
    active
      ? SIDEBAR_ITEM_ACTIVE_CLASS
      : 'text-sidebar-foreground/70 hover:bg-sidebar-accent hover:text-sidebar-foreground'
  }`
}
