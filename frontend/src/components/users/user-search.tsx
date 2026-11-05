import { useUrlSyncedInput } from '@/hooks/use-url-synced-input'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { getUserStatusOptions } from '@/lib/constants/user'
import { m } from '@/paraglide/messages'

interface UserSearchProps {
  email?: string
  phone?: string
  status?: string
  onSearchChange: (email: string | undefined) => void
  onPhoneSearchChange?: (phone: string | undefined) => void
  onStatusChange?: (status: string | undefined) => void
}

export function UserSearch({
  email = '',
  phone = '',
  status,
  onSearchChange,
  onPhoneSearchChange,
  onStatusChange,
}: UserSearchProps) {
  const [searchInput, setSearchInput] = useUrlSyncedInput(email, onSearchChange)
  const [phoneInput, setPhoneInput] = useUrlSyncedInput(phone, onPhoneSearchChange ?? (() => {}))

  return (
    <>
      <Input
        placeholder={m['users.search_placeholder']()}
        value={searchInput}
        onChange={(e) => setSearchInput(e.target.value)}
        data-testid="users-search-input"
        className="max-w-xs"
      />

      {onPhoneSearchChange && (
        <Input
          placeholder={m['users.phone_search_placeholder']()}
          value={phoneInput}
          onChange={(e) => setPhoneInput(e.target.value)}
          data-testid="users-phone-search-input"
          className="max-w-xs"
        />
      )}

      {onStatusChange && (
        <Select
          value={status ?? 'all'}
          onValueChange={(value) => onStatusChange(value === 'all' ? undefined : value)}
        >
          <SelectTrigger data-testid="users-status-filter" className="w-[160px]">
            <SelectValue placeholder={m['users.filter_all_statuses']()} />
          </SelectTrigger>
          <SelectContent>
            {getUserStatusOptions().map((option) => (
              <SelectItem key={option.value} value={option.value}>
                {option.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      )}
    </>
  )
}
