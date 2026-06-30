import { useEffect, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { useAppStore } from '../../stores/appStore'

// Mirror of the backend speech VAD threshold (normalized RMS 0..1): below this a
// sample is treated as silence and does NOT consume the recording budget.
const SPEECH_RMS = 0.008
// How often we sample the mic level to accumulate voiced time.
const TICK_MS = 250
// Wall-clock safety ceiling (× the budget) so a silent/dead mic — where voiced
// time would never reach the cap — can't record indefinitely.
const WALL_SAFETY_FACTOR = 4

export function DurationTimer() {
  const pipelineState = useAppStore((s) => s.pipelineState)
  const maxSeconds = useAppStore((s) => s.config.max_recording_seconds)
  const setRecordingProgress = useAppStore((s) => s.setRecordingProgress)
  const setRecordingDuration = useAppStore((s) => s.setRecordingDuration)
  // Seconds of actual speech (silence excluded) — this is what the cap applies to.
  const [voiced, setVoiced] = useState(0)
  const voicedRef = useRef(0)
  const wallRef = useRef(0)
  const stoppedRef = useRef(false)

  useEffect(() => {
    if (pipelineState !== 'recording') {
      // Stash the voiced seconds just recorded so the Transcribing progress bar
      // can estimate how much text to expect. Only when we actually captured
      // something — don't clobber it with 0 on unrelated state changes.
      if (voicedRef.current > 0) {
        setRecordingDuration(voicedRef.current)
      }
      setVoiced(0)
      voicedRef.current = 0
      wallRef.current = 0
      stoppedRef.current = false
      setRecordingProgress(0)
      return
    }
    const step = TICK_MS / 1000
    const interval = setInterval(() => {
      wallRef.current += step
      // Only count the tick toward the budget if the user is actually speaking.
      if (useAppStore.getState().audioVolume >= SPEECH_RMS) {
        voicedRef.current += step
        setVoiced(voicedRef.current)
        setRecordingProgress(Math.min(1, voicedRef.current / maxSeconds))
      }
      if (
        !stoppedRef.current &&
        (voicedRef.current >= maxSeconds || wallRef.current >= maxSeconds * WALL_SAFETY_FACTOR)
      ) {
        stoppedRef.current = true
        invoke('stop_recording').catch((e: unknown) => {
          console.error('Failed to auto-stop recording at max duration:', e)
        })
      }
    }, TICK_MS)
    return () => clearInterval(interval)
  }, [pipelineState, maxSeconds, setRecordingProgress, setRecordingDuration])

  const total = Math.floor(voiced)
  const mm = String(Math.floor(total / 60)).padStart(2, '0')
  const ss = String(total % 60).padStart(2, '0')

  return (
    <span className="text-[11px] font-mono text-white/80 tabular-nums">
      {mm}:{ss}
    </span>
  )
}
