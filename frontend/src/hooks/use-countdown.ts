import { useEffect, useRef, useState } from 'react'

/**
 * Countdown timer (seconds remaining), `null` while none is active.
 * `start(seconds)` clears any active countdown first; a non-positive value
 * clears without restarting.
 */
export function useCountdown() {
  const [countdown, setCountdown] = useState<number | null>(null)
  const countdownRef = useRef<ReturnType<typeof setInterval> | null>(null)
  // Mutation callbacks can fire after unmount (in-flight request resolves
  // while the caller is gone); starting a countdown then would create an
  // interval nobody will ever clear.
  const mountedRef = useRef(true)

  function clearCountdown() {
    if (countdownRef.current) {
      clearInterval(countdownRef.current)
      countdownRef.current = null
    }
    setCountdown(null)
  }

  function startCountdown(seconds: number) {
    if (!mountedRef.current) return
    clearCountdown()
    if (!seconds || seconds <= 0) return
    setCountdown(seconds)
    countdownRef.current = setInterval(() => {
      setCountdown((prev) => {
        if (prev === null) return null
        if (prev <= 1) {
          if (countdownRef.current) {
            clearInterval(countdownRef.current)
            countdownRef.current = null
          }
          return null
        }
        return prev - 1
      })
    }, 1000)
  }

  useEffect(() => {
    // Re-arm on re-mount (StrictMode dev double-invoke runs cleanup first).
    mountedRef.current = true
    return () => {
      mountedRef.current = false
      clearCountdown()
    }
  }, [])

  return { countdown, startCountdown, clearCountdown }
}
