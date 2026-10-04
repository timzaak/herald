import { useUrlSyncedInput } from '@/hooks/use-url-synced-input'
import { Input } from '@/components/ui/input'
import { m } from '@/paraglide/messages'

interface RealmSearchProps {
  realmId?: string
  onSearchChange: (realmId: string | undefined) => void
}

export function RealmSearch({ realmId = '', onSearchChange }: RealmSearchProps) {
  const [searchInput, setSearchInput] = useUrlSyncedInput(realmId, onSearchChange)

  return (
    <Input
      placeholder={m['realms.search_placeholder']()}
      value={searchInput}
      onChange={(e) => setSearchInput(e.target.value)}
      data-testid="realms-search-input"
    />
  )
}
