import { useEffect, useRef } from 'react'
import { Check, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { useAppStore } from '../../stores/appStore'
import {
  confirmPreview,
  cancelPreview,
  setPreviewText as pushPreviewText,
  setPreviewCaret as pushPreviewCaret,
} from '../../lib/tauri'

// The backend works in Unicode code-point offsets; the DOM/textarea works in
// UTF-16 indices. Convert both ways so multi-unit chars (emoji) stay correct.
function toCodePoints(value: string, uiIndex: number): number {
  return Array.from(value.slice(0, uiIndex)).length
}
function fromCodePoints(value: string, cp: number): number {
  const chars = Array.from(value)
  return chars.slice(0, Math.min(cp, chars.length)).join('').length
}

const WINDOW_WIDTH = 400
const WINDOW_HEIGHT = 260

/**
 * Editable preview shown between polish and output. Unlike the capsule, this is
 * its own NORMAL focusable window, so it hosts a real <textarea> with a native
 * caret and full mouse + keyboard editing. Voice editing still works in
 * parallel (non-command speech is inserted at the caret and re-polished by the
 * backend loop). Confirm via global hotkey / ✓ / Enter / "send"; cancel via
 * Esc / ✕. On confirm/cancel the backend re-activates the original target app
 * before output, so stealing focus here is safe.
 */
export function PreviewWindow() {
  const { t } = useTranslation()
  const previewText = useAppStore((s) => s.previewText)
  const previewCaret = useAppStore((s) => s.previewCaret)
  const setPreviewTextStore = useAppStore((s) => s.setPreviewText)
  const listening = useAppStore((s) => s.previewListening)
  const thinking = useAppStore((s) => s.previewThinking)
  const pipelineState = useAppStore((s) => s.pipelineState)
  const textareaRef = useRef<HTMLTextAreaElement>(null)

  const isPreviewing = pipelineState === 'previewing'

  // Show / position / focus this window while previewing; hide otherwise. The
  // window is created hidden in tauri.conf.json and only ever surfaces here.
  useEffect(() => {
    let cancelled = false
    import('@tauri-apps/api/window')
      .then(
        async ({
          getCurrentWindow,
          LogicalSize,
          LogicalPosition,
          currentMonitor,
          primaryMonitor,
        }) => {
          if (cancelled) return
          const win = getCurrentWindow()
          if (!isPreviewing) {
            await win.hide().catch(() => {})
            return
          }
          await win.setSize(new LogicalSize(WINDOW_WIDTH, WINDOW_HEIGHT)).catch(() => {})
          const monitor =
            (await currentMonitor().catch(() => null)) ?? (await primaryMonitor().catch(() => null))
          if (monitor) {
            const sw = monitor.size.width / monitor.scaleFactor
            const sh = monitor.size.height / monitor.scaleFactor
            const x = Math.round(sw / 2 - WINDOW_WIDTH / 2)
            // Sit above the capsule's bottom-center spot so the two don't overlap
            // (the capsule stays put as the voice-edit mic indicator below).
            const y = Math.round(sh - WINDOW_HEIGHT - 150)
            await win.setPosition(new LogicalPosition(x, y)).catch(() => {})
          }
          await win.show().catch(() => {})
          await win.setFocus().catch(() => {})
        },
      )
      .catch(() => {})
    return () => {
      cancelled = true
    }
  }, [isPreviewing])

  // Drive the textarea caret from the backend's caret offset. This fires on
  // open (backend emits caret = end of text) and after every voice edit (caret
  // = right after the inserted/edited content) — NOT on manual typing, since
  // local edits don't change previewCaret in the store, so the native caret is
  // left untouched. previewCaret is a Unicode code-point offset → UTF-16 index.
  useEffect(() => {
    if (!isPreviewing) return
    const el = textareaRef.current
    if (!el) return
    el.focus()
    const idx = fromCodePoints(el.value, previewCaret)
    el.setSelectionRange(idx, idx)
  }, [isPreviewing, previewCaret])

  const pushCaretFromSelection = () => {
    const el = textareaRef.current
    if (!el) return
    pushPreviewCaret(toCodePoints(el.value, el.selectionStart)).catch(() => {})
  }

  const handleChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    const value = e.target.value
    setPreviewTextStore(value)
    pushPreviewText(value).catch(() => {})
  }

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      confirmPreview().catch(() => {})
    } else if (e.key === 'Escape') {
      e.preventDefault()
      cancelPreview().catch(() => {})
    }
  }

  if (!isPreviewing) return null

  return (
    <div className="w-screen h-screen flex flex-col gap-1.5 rounded-2xl bg-neutral-900/95 text-white shadow-xl ring-1 ring-white/10 p-3">
      {/* Header: title + live state */}
      <div className="flex items-center justify-between gap-2 shrink-0">
        <span className="text-[11px] font-semibold tracking-wide text-white/80">
          {t('capsule.preview.title')}
        </span>
        <span className="text-[10px] flex items-center gap-1 text-white/55">
          {thinking ? (
            <>
              <span className="w-1.5 h-1.5 rounded-full bg-amber-400 animate-pulse" />
              {t('capsule.preview.thinking')}
            </>
          ) : listening ? (
            <>
              <span className="w-1.5 h-1.5 rounded-full bg-emerald-400 animate-pulse" />
              {t('capsule.preview.listening')}
            </>
          ) : null}
        </span>
      </div>

      {/* Real editable textarea — native caret, mouse + keyboard. */}
      <textarea
        ref={textareaRef}
        value={previewText}
        onChange={handleChange}
        onKeyDown={handleKeyDown}
        onSelect={pushCaretFromSelection}
        onClick={pushCaretFromSelection}
        placeholder={t('capsule.preview.empty')}
        spellCheck={false}
        className="flex-1 min-h-0 w-full resize-none bg-transparent outline-none text-[13px] leading-relaxed placeholder:text-white/40"
      />

      {/* Footer: instructions + actions */}
      <div className="flex items-end justify-between gap-2 shrink-0">
        <span className="text-[10px] leading-snug text-white/45">
          {t('capsule.preview.editHint')}
        </span>
        <div className="flex items-center gap-2 shrink-0">
          <button
            type="button"
            onClick={() => cancelPreview().catch(() => {})}
            title={t('capsule.preview.cancel')}
            className="flex items-center justify-center w-7 h-7 rounded-full bg-white/10 hover:bg-white/20 transition-colors"
          >
            <X size={15} />
          </button>
          <button
            type="button"
            onClick={() => confirmPreview().catch(() => {})}
            title={t('capsule.preview.send')}
            className="flex items-center justify-center w-7 h-7 rounded-full bg-emerald-500 hover:bg-emerald-400 transition-colors"
          >
            <Check size={15} />
          </button>
        </div>
      </div>
    </div>
  )
}
