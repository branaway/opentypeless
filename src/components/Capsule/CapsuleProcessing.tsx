import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { motion, useReducedMotion } from 'framer-motion'
import { Loader2, X, FastForward } from 'lucide-react'
import { abortRecording } from '../../lib/tauri'
import { useAppStore } from '../../stores/appStore'

// Rough chars-per-voiced-second used to estimate the total transcript length
// from how long the user spoke. Mixed zh/en speech lands around here; it only
// sets the bar's denominator, so being approximate is fine — the bar caps below
// 100% and snaps to done when the result actually lands.
const CHARS_PER_SECOND = 3.5
// Never estimate fewer than this many chars, so very short clips don't make the
// bar jump straight to the cap on the first token.
const MIN_ESTIMATED_CHARS = 24
// Hard ceiling for the in-progress bar; the final 100% comes from the state
// leaving Transcribing, so the bar never claims "done" before it is.
const PROGRESS_CAP = 0.95

/// Smoothly-eased 0..1 transcribe progress. Targets chars/estimatedTotal (real,
/// response-driven) and eases the displayed value toward it so it only ever
/// moves forward, never snapping backward when the estimate is off.
function useTranscribeProgress(): number {
  const chars = useAppStore((s) => s.transcribeChars)
  const recordingDuration = useAppStore((s) => s.recordingDuration)
  const [display, setDisplay] = useState(0)
  const displayRef = useRef(0)

  useEffect(() => {
    const estimatedTotal = Math.max(MIN_ESTIMATED_CHARS, recordingDuration * CHARS_PER_SECOND)
    const interval = setInterval(() => {
      // Before any token arrives, creep gently toward a small floor so the bar
      // shows life; once tokens stream in, track the real fraction.
      const target =
        chars > 0
          ? Math.min(PROGRESS_CAP, chars / estimatedTotal)
          : Math.min(0.12, displayRef.current + 0.01)
      // Ease ~25% of the remaining gap each tick — fast enough to feel live,
      // smooth enough to avoid jumps. Monotonic: never decreases.
      const next = Math.max(
        displayRef.current,
        displayRef.current + (target - displayRef.current) * 0.25,
      )
      if (Math.abs(next - displayRef.current) > 0.0005) {
        displayRef.current = next
        setDisplay(next)
      }
    }, 60)
    return () => clearInterval(interval)
  }, [chars, recordingDuration])

  return display
}

export function CapsuleProcessing() {
  const { t } = useTranslation()
  const partialTranscript = useAppStore((s) => s.partialTranscript)
  const skipArmed = useAppStore((s) => s.previewSkipArmed)
  const reduced = useReducedMotion()
  const progress = useTranscribeProgress()

  const displayText = partialTranscript || t('capsule.transcribing')

  const handleCancel = async (e: React.MouseEvent) => {
    e.stopPropagation()
    try {
      await abortRecording()
    } catch (err) {
      console.error('Failed to abort processing:', err)
    }
  }

  const stopPointerPropagation = (e: React.PointerEvent) => {
    e.stopPropagation()
  }

  return (
    <motion.div className="relative z-10 flex items-center gap-2 h-9 px-3">
      {/* Shimmer sweep overlay */}
      <div className="capsule-shimmer" />
      <motion.div
        className="flex-shrink-0"
        animate={reduced ? undefined : { rotate: 360 }}
        transition={{ repeat: Infinity, duration: 1, ease: 'linear' }}
      >
        <Loader2 size={12} className="text-white/80" />
      </motion.div>
      <p className="text-[11px] text-white leading-snug truncate flex-1 min-w-0">
        {displayText}
        <motion.span
          className="inline-block w-[2px] h-[11px] bg-white/60 ml-0.5 align-middle"
          animate={reduced ? undefined : { opacity: [1, 0, 1] }}
          transition={{ repeat: Infinity, duration: 0.8 }}
        />
      </p>
      {skipArmed && (
        <FastForward
          size={12}
          className="flex-shrink-0 text-white/90"
          aria-label={t('capsule.preview.skipArmed')}
        />
      )}
      <button
        onPointerDown={stopPointerPropagation}
        onPointerUp={stopPointerPropagation}
        onClick={handleCancel}
        aria-label={t('capsule.cancelProcessing')}
        className="flex-shrink-0 p-1 rounded-full text-white/70 hover:text-white hover:bg-white/15 transition-colors bg-transparent border-none cursor-pointer"
      >
        <X size={12} />
      </button>

      {/* Live transcribe progress: fills as text streams back from the model,
          using the recording length to estimate the total. Caps below 100%;
          the capsule leaving this state is the real completion. */}
      <div
        className="absolute bottom-[2px] left-3 right-3 h-[3px] rounded-full bg-white/10 overflow-hidden pointer-events-none"
        aria-hidden
      >
        <div
          className="h-full rounded-full bg-emerald-400 transition-[width] duration-200 ease-out"
          style={{
            width: `${Math.min(100, progress * 100)}%`,
            boxShadow: '0 0 6px rgba(52,211,153,0.7)',
          }}
        />
      </div>
    </motion.div>
  )
}
