import { useState } from 'react'
import { Check, ChevronsUpDown } from 'lucide-react'
import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from '@/components/ui/command'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { m } from '@/paraglide/messages'

interface ClientAppOption {
  id: string
  name: string
  clientId: string
  /** Present when the caller surfaces the app's enabled state; disabled
   * apps are marked so keys are not unknowingly bound to a dead gate. */
  enabled?: boolean
}

interface ClientAppSelectorProps {
  clientApps: ClientAppOption[]
  value: string | undefined
  onChange: (value: string) => void
  disabled?: boolean
}

export function ClientAppSelector({
  clientApps,
  value,
  onChange,
  disabled = false,
}: ClientAppSelectorProps) {
  const [open, setOpen] = useState(false)
  const selected = clientApps.find((app) => app.id === value)

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          variant="outline"
          role="combobox"
          aria-expanded={open}
          disabled={disabled}
          className="w-full justify-between"
          data-testid="client-app-selector-trigger"
        >
          <span className={cn('truncate', !selected && 'text-muted-foreground')}>
            {selected ? `${selected.name} (${selected.clientId})` : m['shared.select_client_app']()}
          </span>
          <ChevronsUpDown className="ml-2 h-4 w-4 shrink-0 opacity-50" />
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-full p-0" align="start">
        <Command>
          <CommandInput
            placeholder={m['shared.search_client_apps']()}
            data-testid="client-app-selector-search"
          />
          <CommandList>
            <CommandEmpty>{m['shared.no_client_apps_found']()}</CommandEmpty>
            <CommandGroup>
              {clientApps.map((app) => (
                <CommandItem
                  key={app.id}
                  value={`${app.name} ${app.clientId}`}
                  onSelect={() => {
                    onChange(app.id)
                    setOpen(false)
                  }}
                  data-testid={`client-app-selector-item-${app.id}`}
                >
                  <Check
                    className={cn('mr-2 h-4 w-4', value === app.id ? 'opacity-100' : 'opacity-0')}
                  />
                  <span
                    className={cn('truncate', app.enabled === false && 'text-muted-foreground')}
                  >
                    {app.name} ({app.clientId})
                  </span>
                  {app.enabled === false && (
                    <span
                      className="ml-2 shrink-0 text-xs text-muted-foreground"
                      data-testid={`client-app-selector-disabled-${app.id}`}
                    >
                      {m['client_apps.status_disabled_label']()}
                    </span>
                  )}
                </CommandItem>
              ))}
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  )
}
