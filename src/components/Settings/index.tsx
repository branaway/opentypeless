import { useState, useEffect } from 'react'
import { AnimatePresence, motion } from 'framer-motion'
import { useTranslation } from 'react-i18next'
import { useAppStore } from '../../stores/appStore'
import { SettingsSidebar, type PaneId } from './SettingsSidebar'
import { GeneralPane } from './GeneralPane'
import { SttPane } from './SttPane'
import { LlmPane } from './LlmPane'
import { DictionaryPane } from './DictionaryPane'
import { ScenesPane } from './ScenesPane'
import { UsagePane } from './UsagePane'
import { AboutPane } from './AboutPane'
import { DirtyBar, useDirtyConfig } from './shared/DirtyBar'

const paneTitleKeys: Record<PaneId, string> = {
  general: 'settings.general',
  stt: 'settings.speechRecognition',
  llm: 'settings.aiPolish',
  dictionary: 'settings.dictionary',
  scenes: 'settings.scenes',
  usage: 'settings.usage',
  about: 'settings.about',
}

const VALID_PANES: PaneId[] = ['general', 'stt', 'llm', 'dictionary', 'scenes', 'usage', 'about']

function paneFromHash(): PaneId {
  const seg = window.location.hash.replace(/^#\/settings\/?/, '')
  return (VALID_PANES as string[]).includes(seg) ? (seg as PaneId) : 'general'
}

export function Settings() {
  const [activePane, setActivePane] = useState<PaneId>(paneFromHash)
  const config = useAppStore((s) => s.config)
  const setSavedConfig = useAppStore((s) => s.setSavedConfig)
  const isDirty = useDirtyConfig()
  const { t } = useTranslation()

  // Snapshot config when settings opens
  useEffect(() => {
    setSavedConfig(config)
  }, []) // eslint-disable-line react-hooks/exhaustive-deps

  // Honor deep links to a specific pane (e.g. #/settings/usage from the capsule).
  useEffect(() => {
    const onHash = () => setActivePane(paneFromHash())
    window.addEventListener('hashchange', onHash)
    return () => window.removeEventListener('hashchange', onHash)
  }, [])

  return (
    <div className="w-full h-full bg-bg-primary text-text-primary flex flex-col">
      <div className="flex-1 flex min-h-0">
        {/* Sidebar */}
        <SettingsSidebar activePane={activePane} onSelect={setActivePane} />

        {/* Content */}
        <div className="flex-1 flex flex-col min-w-0">
          {/* Title bar */}
          <div className="flex items-center justify-between px-6 pt-4 pb-3 border-b border-border bg-bg-primary/50">
            <h2 className="text-[15px] font-medium">{t(paneTitleKeys[activePane])}</h2>
          </div>

          {/* Pane content */}
          <div className="flex-1 overflow-y-auto px-6 py-5">
            <AnimatePresence mode="sync">
              <motion.div
                key={activePane}
                className="w-full"
                initial={{ opacity: 0, y: 6 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: -6 }}
                transition={{ duration: 0.1, ease: 'easeOut' }}
              >
                {activePane === 'general' && <GeneralPane />}
                {activePane === 'stt' && <SttPane />}
                {activePane === 'llm' && <LlmPane />}
                {activePane === 'dictionary' && <DictionaryPane />}
                {activePane === 'scenes' && <ScenesPane />}
                {activePane === 'usage' && <UsagePane />}
                {activePane === 'about' && <AboutPane />}
              </motion.div>
            </AnimatePresence>
          </div>
        </div>
      </div>

      {/* Dirty bar */}
      <AnimatePresence>{isDirty && <DirtyBar />}</AnimatePresence>
    </div>
  )
}
