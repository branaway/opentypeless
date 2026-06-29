import { useEffect, useState, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { RefreshCw, Trash2 } from 'lucide-react'
import { getUsageSummary, clearUsage, type UsageSummary } from '../../lib/tauri'

function formatMoney(amount: number, currency: string): string {
  const symbol = currency === 'CNY' ? '¥' : ''
  // Sub-cent precision so light usage doesn't all read as ¥0.00.
  const digits = amount > 0 && amount < 0.1 ? 4 : 2
  return `${symbol}${amount.toFixed(digits)}`
}

function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`
  return `${n}`
}

export function UsagePane() {
  const { t } = useTranslation()
  const [summary, setSummary] = useState<UsageSummary | null>(null)
  const [loading, setLoading] = useState(true)

  const refresh = useCallback(() => {
    setLoading(true)
    getUsageSummary()
      .then(setSummary)
      .catch((e) => console.error('Failed to load usage summary:', e))
      .finally(() => setLoading(false))
  }, [])

  useEffect(() => {
    refresh()
  }, [refresh])

  const handleClear = useCallback(async () => {
    if (!window.confirm(t('settings.usageClearConfirm'))) return
    try {
      await clearUsage()
      refresh()
    } catch (e) {
      console.error('Failed to clear usage:', e)
    }
  }, [refresh, t])

  const currency = summary?.currency ?? 'CNY'

  return (
    <div className="space-y-5 text-[13px]">
      <div className="flex items-center justify-between">
        <p className="text-text-secondary leading-relaxed">{t('settings.usageDescription')}</p>
        <button
          onClick={refresh}
          aria-label={t('settings.usageRefresh')}
          className="shrink-0 ml-3 p-2 rounded-[8px] bg-bg-secondary border border-border text-text-secondary hover:text-text-primary cursor-pointer"
        >
          <RefreshCw size={14} className={loading ? 'animate-spin' : ''} />
        </button>
      </div>

      {/* Spend headline cards */}
      <div className="grid grid-cols-3 gap-3">
        <StatCard
          label={t('settings.usageToday')}
          value={formatMoney(summary?.today_cost ?? 0, currency)}
        />
        <StatCard
          label={t('settings.usageMonth')}
          value={formatMoney(summary?.month_cost ?? 0, currency)}
          highlight
        />
        <StatCard
          label={t('settings.usageTotal')}
          value={formatMoney(summary?.total_cost ?? 0, currency)}
        />
      </div>

      {/* Token / call breakdown */}
      <SectionCard title={t('settings.usageBreakdown')}>
        <InfoRow label={t('settings.usageCalls')} value={`${summary?.total_calls ?? 0}`} />
        <InfoRow
          label={t('settings.usageAudioTokens')}
          value={formatTokens(summary?.audio_tokens ?? 0)}
        />
        <InfoRow
          label={t('settings.usageInputTokens')}
          value={formatTokens(summary?.input_tokens ?? 0)}
        />
        <InfoRow
          label={t('settings.usageOutputTokens')}
          value={formatTokens(summary?.output_tokens ?? 0)}
        />
      </SectionCard>

      <p className="text-[11px] text-text-tertiary leading-relaxed">
        {t('settings.usagePricingNote')}
      </p>

      <button
        onClick={handleClear}
        className="flex items-center gap-1.5 px-3 py-2 rounded-[8px] bg-transparent border border-border text-text-secondary hover:text-error hover:border-error/40 cursor-pointer text-[13px]"
      >
        <Trash2 size={13} /> {t('settings.usageClear')}
      </button>
    </div>
  )
}

function StatCard({
  label,
  value,
  highlight,
}: {
  label: string
  value: string
  highlight?: boolean
}) {
  return (
    <div
      className={`rounded-[10px] border p-3 ${
        highlight ? 'border-accent/40 bg-accent/5' : 'border-border bg-bg-secondary/50'
      }`}
    >
      <div className="text-[11px] text-text-tertiary">{label}</div>
      <div
        className={`mt-1 text-[20px] font-semibold ${highlight ? 'text-accent' : 'text-text-primary'}`}
      >
        {value}
      </div>
    </div>
  )
}

function SectionCard({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="border border-border rounded-[10px] overflow-hidden">
      <div className="px-3 py-2.5 bg-bg-secondary/50 border-b border-border">
        <h3 className="text-[13px] font-medium text-text-primary">{title}</h3>
      </div>
      {children}
    </div>
  )
}

function InfoRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex justify-between px-3 py-2.5 border-b border-border last:border-b-0">
      <span className="text-text-secondary">{label}</span>
      <span className="text-text-primary tabular-nums">{value}</span>
    </div>
  )
}
