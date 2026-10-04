import { useEffect, useState } from 'react'
import type { Dispatch, SetStateAction } from 'react'
import { useDebounce } from '@/hooks/use-debounce'

/**
 * Input state for a URL-driven filter box (users email, realms id, …).
 *
 * The box LEADS the URL only while the user types: edits are debounced
 * (500ms) and reported via onCommit as `string | undefined` (empty →
 * undefined, i.e. filter removed). When `urlValue` changes externally (deep
 * link, back/forward, programmatic navigation) the box must follow it — a
 * bare useState(urlValue) seed ignores later prop changes and leaves the box
 * showing a stale filter. The follow adjusts state during render (guarded,
 * the React-documented pattern) rather than in an effect, and skips while
 * the user is mid-edit so a debounced round-trip never clobbers uncommitted
 * keystrokes.
 *
 * Returns the [value, setValue] pair to bind to <Input value/onChange>.
 */
export function useUrlSyncedInput(
  urlValue: string,
  onCommit: (value: string | undefined) => void
): [string, Dispatch<SetStateAction<string>>] {
  const [input, setInput] = useState(urlValue)
  const debounced = useDebounce(input, 500)

  const [lastUrlValue, setLastUrlValue] = useState(urlValue)
  if (urlValue !== lastUrlValue) {
    if (input === lastUrlValue) {
      setInput(urlValue)
    }
    setLastUrlValue(urlValue)
  }

  useEffect(() => {
    if (debounced !== urlValue) {
      onCommit(debounced || undefined)
    }
  }, [debounced, urlValue, onCommit])

  return [input, setInput]
}
