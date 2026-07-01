import { useEffect, useRef } from 'react'
import { useAppStore, type PipelineState } from '../stores/appStore'

interface CapsuleSize {
  width: number
  height: number
}

export interface CapsuleVisibilityInput {
  capsuleAutoHide: boolean
  contextMenuOpen: boolean
  capsuleExpanded: boolean
  hasError: boolean
  pipelineState: PipelineState
}

export function getCapsuleVisibility({
  capsuleAutoHide,
  contextMenuOpen,
  capsuleExpanded,
  hasError,
  pipelineState,
}: CapsuleVisibilityInput): boolean {
  // While previewing, the editable text lives in the separate focusable preview
  // window, but the capsule STAYS in its usual spot as the mic indicator for
  // hands-free voice editing — so it must remain visible (pipelineState !== idle
  // already covers this).
  return (
    !capsuleAutoHide || contextMenuOpen || capsuleExpanded || hasError || pipelineState !== 'idle'
  )
}

// The capsule is always non-focusable: it's a transparent always-on-top panel
// that must never steal keyboard focus from the app being dictated into. The
// editable preview lives in its own dedicated focusable window instead (see
// components/Preview/PreviewWindow).
export function getCapsuleFocusable(): boolean {
  return false
}

function getSizeForState(
  state: PipelineState,
  expanded: boolean,
  hasError: boolean,
  contextMenuOpen: boolean,
): CapsuleSize {
  if (contextMenuOpen) return { width: 220, height: 220 }
  if (hasError) return { width: 200, height: 36 }
  if (expanded) return { width: 220, height: 90 }
  switch (state) {
    case 'idle':
      return { width: 36, height: 36 }
    case 'recording':
      return { width: 200, height: 36 }
    case 'transcribing':
    case 'polishing':
      return { width: 220, height: 36 }
    case 'outputting':
      return { width: 120, height: 36 }
    case 'previewing':
      // Mic-indicator pill for voice editing; the editable text is in the
      // separate preview window. Same footprint as the recording pill.
      return { width: 200, height: 36 }
    default:
      return { width: 36, height: 36 }
  }
}

export function useCapsuleResize() {
  const pipelineState = useAppStore((s) => s.pipelineState)
  const capsuleExpanded = useAppStore((s) => s.capsuleExpanded)
  const pipelineError = useAppStore((s) => s.pipelineError)
  const contextMenuOpen = useAppStore((s) => s.contextMenuOpen)
  const setContextMenuReady = useAppStore((s) => s.setContextMenuReady)
  const capsuleAutoHide = useAppStore((s) => s.config.capsule_auto_hide)
  const initialized = useRef(false)
  const prevWindowSize = useRef<{ width: number; height: number } | null>(null)

  const hasError = pipelineError !== null

  useEffect(() => {
    const size = getSizeForState(pipelineState, capsuleExpanded, hasError, contextMenuOpen)
    const windowWidth = size.width + 24
    const windowHeight = size.height + 24
    const shouldShow = getCapsuleVisibility({
      capsuleAutoHide,
      contextMenuOpen,
      capsuleExpanded,
      hasError,
      pipelineState,
    })

    import('@tauri-apps/api/window')
      .then(
        async ({
          getCurrentWindow,
          LogicalSize,
          LogicalPosition,
          currentMonitor,
          primaryMonitor,
        }) => {
          // currentMonitor() can return null for a hidden/transparent window at
          // startup; fall back to the primary monitor so we can always center.
          const resolveMonitor = async () =>
            (await currentMonitor().catch(() => null)) ?? (await primaryMonitor().catch(() => null))
          const win = getCurrentWindow()
          await win.setFocusable(getCapsuleFocusable()).catch(() => {})

          if (!initialized.current) {
            // First mount: position on the monitor the capsule window actually
            // ends up on (which tracks wherever the OS opens it, typically near
            // the user's active window/focus), not always the OS-designated
            // primary monitor.
            await win.setSize(new LogicalSize(windowWidth, windowHeight)).catch(() => {})
            try {
              const monitor = await resolveMonitor()
              if (monitor) {
                const scale = monitor.scaleFactor
                const monX = monitor.position.x / scale
                const monY = monitor.position.y / scale
                const sw = monitor.size.width / scale
                const sh = monitor.size.height / scale
                const x = monX + Math.round(sw / 2 - windowWidth / 2)
                const y = monY + Math.round(sh - windowHeight - 80)
                await win.setPosition(new LogicalPosition(x, y)).catch(() => {})
              }
            } catch {
              /* ignore – monitor info unavailable */
            }
            if (shouldShow) {
              await win.show().catch(() => {})
            } else {
              await win.hide().catch(() => {})
            }
            initialized.current = true
            prevWindowSize.current = { width: windowWidth, height: windowHeight }
            return
          }

          // Subsequent resizes: anchor the BOTTOM edge so the window grows UPWARD
          // (the capsule stays at the bottom and the preview expands above it,
          // never pushing the bottom off-screen). Horizontally: normal states
          // anchor the capsule's CENTER so it stays strictly centered as the width
          // changes; the preview is centered on the screen. Everything is clamped
          // on-screen so repeated triggers can't drift the window away.
          const prev = prevWindowSize.current
          if (prev) {
            const pos = await win.outerPosition().catch(() => null)
            if (pos) {
              const monitor = await resolveMonitor()
              const scale = monitor?.scaleFactor ?? 1
              const oldLeftX = pos.x / scale
              const oldBottomY = pos.y / scale + prev.height
              // Anchor the capsule's CENTER (not its left edge) so the center
              // stays put as the width changes between states — strict center
              // alignment. The context menu keeps its left edge so the capsule
              // doesn't jump sideways when the menu expands to the right.
              const oldCenterX = oldLeftX + prev.width / 2
              let newX = contextMenuOpen
                ? Math.round(oldLeftX)
                : Math.round(oldCenterX - windowWidth / 2)
              let newY = Math.round(oldBottomY - windowHeight)
              if (monitor) {
                const monX = monitor.position.x / scale
                const monY = monitor.position.y / scale
                const sw = monitor.size.width / scale
                const sh = monitor.size.height / scale
                newX = Math.max(monX + 8, Math.min(newX, Math.round(monX + sw - windowWidth - 8)))
                newY = Math.max(monY + 8, Math.min(newY, Math.round(monY + sh - windowHeight - 8)))
              }
              await win.setPosition(new LogicalPosition(newX, newY)).catch(() => {})
              await win.setSize(new LogicalSize(windowWidth, windowHeight)).catch(() => {})
            } else {
              await win.setSize(new LogicalSize(windowWidth, windowHeight)).catch(() => {})
            }
          } else {
            await win.setSize(new LogicalSize(windowWidth, windowHeight)).catch(() => {})
          }

          prevWindowSize.current = { width: windowWidth, height: windowHeight }

          // Signal that the window has finished resizing for context menu
          if (contextMenuOpen) {
            setContextMenuReady(true)
          }

          if (shouldShow) {
            await win.show().catch(() => {})
          } else {
            await win.hide().catch(() => {})
          }
        },
      )
      .catch(() => {})
  }, [
    pipelineState,
    capsuleExpanded,
    hasError,
    contextMenuOpen,
    capsuleAutoHide,
    setContextMenuReady,
  ])

  return getSizeForState(pipelineState, capsuleExpanded, hasError, contextMenuOpen)
}
