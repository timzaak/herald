import { useRef, useState } from 'react'

/**
 * Menu label that reveals the full text in a floating tooltip — but only when
 * the label is actually clipped by the sidebar width. Mouse-only on purpose:
 * keyboard users already get the full text from the link/div content, so we
 * avoid adding redundant focusable tooltip triggers (no extra tab stops).
 *
 * The tooltip is `position: fixed` so it escapes the nav's overflow clipping.
 */
export function SidebarMenuLabel({ label, className }: { label: string; className?: string }) {
  const textRef = useRef<HTMLSpanElement>(null)
  const [coords, setCoords] = useState<{ top: number; left: number } | null>(null)

  const show = () => {
    const el = textRef.current
    if (!el || el.scrollWidth <= el.clientWidth) return
    const rect = el.getBoundingClientRect()
    setCoords({ top: rect.top + rect.height / 2, left: rect.right + 8 })
  }

  return (
    <>
      <span
        ref={textRef}
        className={className}
        onMouseEnter={show}
        onMouseLeave={() => setCoords(null)}
      >
        {label}
      </span>
      {coords && (
        <div
          role="tooltip"
          style={{
            position: 'fixed',
            top: coords.top,
            left: coords.left,
            transform: 'translateY(-50%)',
          }}
          className="pointer-events-none z-50 max-w-[240px] rounded-md bg-primary px-2.5 py-1 text-xs font-medium text-primary-foreground shadow-md animate-in fade-in-0 zoom-in-95"
        >
          {label}
        </div>
      )}
    </>
  )
}
