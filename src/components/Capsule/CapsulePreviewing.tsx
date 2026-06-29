import { motion } from 'framer-motion'
import { Mic } from 'lucide-react'
import { useAppStore } from '../../stores/appStore'
import { Waveform } from './Waveform'

/**
 * Capsule pill shown while the editable preview is open. The editable text lives
 * in the separate, focusable preview window; this capsule stays in its usual
 * spot as the mic indicator for hands-free voice editing — a live waveform while
 * listening, animated dots while a spoken edit is being applied.
 */
export function CapsulePreviewing() {
  const thinking = useAppStore((s) => s.previewThinking)

  return (
    <div className="relative z-10 flex items-center gap-2 h-9 px-3">
      <Mic size={13} className="shrink-0 opacity-90" />
      {thinking ? (
        <span className="flex items-center gap-1">
          {[0, 1, 2].map((i) => (
            <motion.span
              key={i}
              className="w-1.5 h-1.5 rounded-full bg-white/90"
              animate={{ opacity: [0.3, 1, 0.3] }}
              transition={{ duration: 0.9, repeat: Infinity, delay: i * 0.15 }}
            />
          ))}
        </span>
      ) : (
        <Waveform />
      )}
    </div>
  )
}
