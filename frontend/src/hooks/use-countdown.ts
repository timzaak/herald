import { useEffect, useRef, useState } from 'react'

/**
 * Countdown timer (seconds remaining), `null` while none is active.
 * `start(seconds)` clears any active countdown first; a non-positive value
 * clears without restarting.
 */
export function useCountdown() {
  const [countdown, setCountdown] = useState<number | null>(null)
  const countdownRef = useRef<ReturnType<typeof setInterval> | null>(null)

  function clearCountdown() {
    if (countdownRef.current) {
      clearInterval(countdownRef.current)
      countdownRef.current = null
    }
    setCountdown(null)
  }

  function startCountdown(seconds: number) {
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
    return () => clearCountdown()
  }, [])

  return { countdown, startCountdown, clearCountdown }
}
