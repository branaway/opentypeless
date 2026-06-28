import { useEffect, useRef } from 'react'
import { useReducedMotion } from 'framer-motion'
import { useAppStore } from '../../stores/appStore'

const BAR_COUNT = 12
const MIN_HEIGHT = 2
const MAX_HEIGHT = 28

// Phase offsets give each bar an independent rhythm so they move like a real waveform
const PHASES = Array.from({ length: BAR_COUNT }, (_, i) => (i * Math.PI * 2) / BAR_COUNT)

export function Waveform() {
  const barsRef = useRef<(HTMLDivElement | null)[]>([])
  const rafRef = useRef<number>(0)
  const smoothed = useRef<number>(0)
  const reduced = useReducedMotion()

  useEffect(() => {
    if (reduced) {
      barsRef.current.forEach((bar) => {
        if (!bar) return
        bar.style.height = `${(MIN_HEIGHT + MAX_HEIGHT) / 2}px`
        bar.style.opacity = '0.8'
      })
      return
    }

    const animate = () => {
      const raw = useAppStore.getState().audioVolume

      // dB scale calibrated so background noise stays near the floor and only
      // real speech drives the bars. -40 dB (RMS ~0.01) maps to 0, -12 dB
      // (RMS ~0.25, loud speech) maps to 1.
      const db = 20 * Math.log10(Math.max(raw, 0.0001))
      const target = Math.max(0, Math.min(1, (db + 40) / 28))

      // Smooth attack (fast) / decay (slower) so bars snap up but glide down
      const prev = smoothed.current
      smoothed.current = target > prev
        ? prev + (target - prev) * 0.6   // fast attack
        : prev + (target - prev) * 0.12  // slow decay

      const level = smoothed.current
      const t = Date.now()

      barsRef.current.forEach((bar, i) => {
        if (!bar) return
        // Each bar oscillates at its own phase; amplitude of oscillation shrinks as voice gets louder
        const idleWobble = Math.sin(t / 400 + PHASES[i]) * 0.12 * (1 - level)
        // Center bars go tallest — makes a classic waveform arch shape
        const centerBoost = 1 - Math.abs(i - (BAR_COUNT - 1) / 2) / ((BAR_COUNT - 1) / 2) * 0.35
        const normalized = Math.max(0, Math.min(1, (level + idleWobble) * centerBoost))
        const height = MIN_HEIGHT + (MAX_HEIGHT - MIN_HEIGHT) * normalized
        bar.style.height = `${height}px`
        bar.style.opacity = `${Math.max(0.35, 0.4 + normalized * 0.6)}`
      })

      rafRef.current = requestAnimationFrame(animate)
    }

    rafRef.current = requestAnimationFrame(animate)
    return () => cancelAnimationFrame(rafRef.current)
  }, [reduced])

  return (
    <div className="flex items-center justify-center gap-[2px]" style={{ height: `${MAX_HEIGHT}px` }}>
      {Array.from({ length: BAR_COUNT }).map((_, i) => (
        <div
          key={i}
          ref={(el) => { barsRef.current[i] = el }}
          className="rounded-full bg-white"
          style={{
            width: '2px',
            height: `${MIN_HEIGHT}px`,
            opacity: 0.4,
            transition: 'height 40ms ease-out, opacity 40ms ease-out',
          }}
        />
      ))}
    </div>
  )
}
