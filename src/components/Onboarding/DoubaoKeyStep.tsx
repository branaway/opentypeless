import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { CheckCircle2, XCircle, Loader2, ExternalLink, ChevronDown, ChevronUp } from 'lucide-react'
import { useAppStore } from '../../stores/appStore'
import { testLlmConnection } from '../../lib/tauri'

const ARK_CONSOLE_URL = 'https://console.volcengine.com/ark/region:ark+cn-beijing/apiKey'
const DEFAULT_BASE_URL = 'https://ark.cn-beijing.volces.com/api/v3'
const DOUBAO_MODEL = 'doubao-seed-2-0-lite-260428'

export function DoubaoKeyStep() {
  const { t } = useTranslation()
  const config = useAppStore((s) => s.config)
  const updateConfig = useAppStore((s) => s.updateConfig)
  const sttTestStatus = useAppStore((s) => s.sttTestStatus)
  const setSttTestStatus = useAppStore((s) => s.setSttTestStatus)
  const [showAdvanced, setShowAdvanced] = useState(false)

  const baseUrl = config.llm_base_url || DEFAULT_BASE_URL

  const handleTest = async () => {
    if (!config.stt_api_key) return
    setSttTestStatus('testing')
    try {
      const ok = await testLlmConnection(config.stt_api_key, 'doubao', baseUrl, DOUBAO_MODEL)
      setSttTestStatus(ok ? 'success' : 'error')
    } catch {
      setSttTestStatus('error')
    }
  }

  return (
    <div className="space-y-4">
      <div className="px-3 py-3 bg-bg-secondary rounded-[10px] text-[12px] text-text-secondary leading-relaxed">
        {t('onboarding.doubaoKey.desc', 'Doubao Seed 2.0 Lite is an audio-native model — it transcribes and polishes your speech in one API call. No separate transcription service needed.')}
      </div>

      {/* API Key — required */}
      <div>
        <label className="block text-[13px] font-medium text-text-secondary mb-2">
          {t('onboarding.doubaoKey.label', 'Volcengine ARK API Key')}
          <span className="text-error ml-1">*</span>
        </label>
        <div className="flex gap-2">
          <input
            type="password"
            value={config.stt_api_key}
            onChange={(e) => {
              updateConfig({ stt_api_key: e.target.value, llm_api_key: e.target.value })
              setSttTestStatus('idle')
            }}
            placeholder={t('onboarding.doubaoKey.placeholder', 'Paste your ARK API key...')}
            className="flex-1 px-3 py-2.5 bg-bg-secondary border border-border rounded-[10px] text-[13px] text-text-primary outline-none focus:border-border-focus transition-colors"
          />
          <button
            onClick={handleTest}
            disabled={!config.stt_api_key || sttTestStatus === 'testing'}
            className="px-4 py-2.5 bg-accent text-white rounded-[10px] text-[13px] border-none cursor-pointer hover:bg-accent-hover disabled:opacity-40 disabled:cursor-not-allowed transition-colors flex items-center gap-1.5"
          >
            {sttTestStatus === 'testing' && <Loader2 size={14} className="animate-spin" />}
            {t('onboarding.stt.testButton', 'Test')}
          </button>
        </div>

        {sttTestStatus === 'success' && (
          <p className="flex items-center gap-1 text-[12px] text-success mt-2">
            <CheckCircle2 size={13} />
            {t('onboarding.doubaoKey.connectionOk', 'API key verified — Doubao Seed 2.0 Lite is ready')}
          </p>
        )}
        {sttTestStatus === 'error' && (
          <p className="flex items-center gap-1 text-[12px] text-error mt-2">
            <XCircle size={13} />
            {t('onboarding.doubaoKey.connectionFail', 'Could not connect — check your API key')}
          </p>
        )}
      </div>

      <a
        href={ARK_CONSOLE_URL}
        target="_blank"
        rel="noreferrer"
        className="flex items-center gap-1.5 text-[12px] text-accent hover:underline"
      >
        <ExternalLink size={12} />
        {t('onboarding.doubaoKey.getKey', 'Get your ARK API key from Volcengine console')}
      </a>

      {/* Advanced: base URL override */}
      <div>
        <button
          onClick={() => setShowAdvanced((v) => !v)}
          className="flex items-center gap-1 text-[12px] text-text-tertiary hover:text-text-secondary bg-transparent border-none cursor-pointer transition-colors p-0"
        >
          {showAdvanced ? <ChevronUp size={12} /> : <ChevronDown size={12} />}
          {t('onboarding.doubaoKey.advanced', 'Advanced')}
        </button>

        {showAdvanced && (
          <div className="mt-3">
            <label className="block text-[13px] font-medium text-text-secondary mb-2">
              {t('onboarding.doubaoKey.baseUrlLabel', 'Base URL')}
            </label>
            <input
              type="text"
              value={baseUrl}
              onChange={(e) => {
                updateConfig({ llm_base_url: e.target.value })
                setSttTestStatus('idle')
              }}
              placeholder={DEFAULT_BASE_URL}
              className="w-full px-3 py-2.5 bg-bg-secondary border border-border rounded-[10px] text-[13px] text-text-primary outline-none focus:border-border-focus transition-colors font-mono"
            />
            <p className="text-[11px] text-text-tertiary mt-1.5">
              {t('onboarding.doubaoKey.baseUrlHint', 'Change only if using a private deployment or proxy.')}
            </p>
          </div>
        )}
      </div>
    </div>
  )
}
