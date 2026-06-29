import { useTranslation } from 'react-i18next'
import { motion, useReducedMotion } from 'framer-motion'
import { X } from 'lucide-react'
import { abortRecording } from '../../lib/tauri'
import { useAppStore } from '../../stores/appStore'
import { Waveform } from './Waveform'
import { DurationTimer } from './DurationTimer'

export function CapsuleRecording() {
  const { t } = useTranslation()
  const reduced = useReducedMotion()
  const progress = useAppStore((s) => s.recordingProgress)

  const handleCancel = async (e: React.MouseEvent) => {
    e.stopPropagation()
    try {
      await abortRecording()
    } catch (err) {
      console.error('Failed to abort recording:', err)
    }
  }

  const stopPointerPropagation = (e: React.PointerEvent) => {
    e.stopPropagation()
  }

  return (
    <motion.div className="relative z-10 flex items-center gap-2 h-9 px-3">
      {/* White pulse dot — gentle opacity loop */}
      <motion.div
        className="w-2 h-2 rounded-full bg-white/80 flex-shrink-0"
        animate={reduced ? undefined : { opacity: [1, 0.5, 1] }}
        transition={{ repeat: Infinity, duration: 1.5, ease: 'easeInOut' }}
      />
      <Waveform />
      <div className="flex-1" />
      <DurationTimer />
      <button
        onPointerDown={stopPointerPropagation}
        onPointerUp={stopPointerPropagation}
        onClick={handleCancel}
        aria-label={t('capsule.cancelRecording')}
        className="flex-shrink-0 p-1 rounded-full text-white/70 hover:text-white hover:bg-white/15 transition-colors bg-transparent border-none cursor-pointer"
      >
        <X size={12} />
      </button>

      {/* Implicit budget progress: a subtle line along the bottom that fills as
          voiced time approaches the max recording duration. Turns amber near
          the limit so the user notices they're running out. */}
      <div
        className="absolute bottom-[2px] left-3 right-3 h-[2px] rounded-full bg-white/10 overflow-hidden pointer-events-none"
        aria-hidden
      >
        <div
          className={`h-full rounded-full transition-[width] duration-300 ease-linear ${
            progress > 0.85 ? 'bg-amber-300/80' : 'bg-white/45'
          }`}
          style={{ width: `${Math.min(100, progress * 100)}%` }}
        />
      </div>
    </motion.div>
  )
}
