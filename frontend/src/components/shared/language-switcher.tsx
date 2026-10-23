import { Globe } from 'lucide-react'
import { useLocale } from '@/components/shared/locale-provider'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { cn } from '@/lib/utils'
import type { Locale } from '@/paraglide/runtime'

const LOCALE_OPTIONS: ReadonlyArray<{ value: Locale; label: string }> = [
  { value: 'en', label: 'English' },
  { value: 'zh-CN', label: '中文' },
]

export function LanguageSwitcher({ className }: { className?: string }) {
  const { locale, switchLocale } = useLocale()

  return (
    <Select value={locale} onValueChange={(value) => switchLocale(value as Locale)}>
      <SelectTrigger
        data-testid="language-switcher"
        className={cn(
          'h-8 border-sidebar-border bg-transparent px-2.5 text-xs text-sidebar-foreground/70 hover:text-sidebar-foreground',
          className
        )}
      >
        {/* `flex!` overrides the trigger's `[&>span]:line-clamp-1` rule, which
            otherwise forces `display:-webkit-box` and stacks the icon/text. */}
        <span className="flex! items-center gap-1.5">
          <Globe className="size-3.5 opacity-60" />
          <SelectValue />
        </span>
      </SelectTrigger>
      <SelectContent>
        {LOCALE_OPTIONS.map((option) => (
          <SelectItem
            key={option.value}
            value={option.value}
            data-testid={`language-switcher-item-${option.value}`}
          >
            {option.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}
