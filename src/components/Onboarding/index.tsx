import { useEffect } from 'react'
import { AnimatePresence, motion } from 'framer-motion'
import { useTranslation } from 'react-i18next'
import { useAppStore } from '../../stores/appStore'
import { saveOnboardingCompleted, updateConfig as saveConfig } from '../../lib/tauri'
import { OnboardingLayout } from './OnboardingLayout'
import { WelcomeStep } from './WelcomeStep'
import { DoubaoKeyStep } from './DoubaoKeyStep'
import { DoneStep } from './DoneStep'
import { slideRight } from '../../lib/animations'

const TOTAL_STEPS = 3

const ARK_BASE_URL = 'https://ark.cn-beijing.volces.com/api/v3'
const DOUBAO_MODEL = 'doubao-seed-2-0-lite-260428'

export function Onboarding() {
  const { t } = useTranslation()
  const step = useAppStore((s) => s.onboardingStep)
  const setStep = useAppStore((s) => s.setOnboardingStep)
  const setOnboardingCompleted = useAppStore((s) => s.setOnboardingCompleted)
  const sttTestStatus = useAppStore((s) => s.sttTestStatus)
  const updateConfig = useAppStore((s) => s.updateConfig)
  const config = useAppStore((s) => s.config)

  // Pre-wire Doubao Audio as the provider on mount so the rest of the app
  // is correctly configured before the user even hits the key step.
  useEffect(() => {
    updateConfig({
      stt_provider: 'doubao-audio',
      llm_provider: 'doubao',
      llm_base_url: ARK_BASE_URL,
      llm_model: DOUBAO_MODEL,
      polish_enabled: false, // doubao-audio transcribes + polishes in one shot
    })
  }, []) // eslint-disable-line react-hooks/exhaustive-deps

  const canNext = (() => {
    switch (step) {
      case 0: return true
      case 1: return sttTestStatus === 'success'
      case 2: return true
      default: return false
    }
  })()

  const titles = [
    { title: t('onboarding.steps.welcome'), subtitle: t('onboarding.steps.welcomeSub') },
    {
      title: t('onboarding.doubaoKey.stepTitle', 'ARK API Key'),
      subtitle: t('onboarding.doubaoKey.stepSubtitle', 'One key for transcription and polishing'),
    },
    { title: t('onboarding.steps.setupComplete'), subtitle: undefined },
  ]

  const handleNext = async () => {
    if (step < TOTAL_STEPS - 1) {
      try {
        await saveConfig(config)
      } catch {
        // Best-effort save
      }
      setStep(step + 1)
    } else {
      await saveConfig(config)
      await saveOnboardingCompleted()
      setOnboardingCompleted(true)
    }
  }

  const handleBack = async () => {
    if (step > 0) {
      try {
        await saveConfig(config)
      } catch {
        // Best-effort save
      }
      setStep(step - 1)
    }
  }

  // Skip is only available on Welcome and Done — not the key step
  const handleSkip = step !== 1
    ? async () => {
        await saveConfig(config)
        await saveOnboardingCompleted()
        setOnboardingCompleted(true)
      }
    : undefined

  return (
    <OnboardingLayout
      step={step}
      totalSteps={TOTAL_STEPS}
      title={titles[step].title}
      subtitle={titles[step].subtitle}
      canNext={canNext}
      canBack={step > 0}
      nextLabel={
        step === TOTAL_STEPS - 1 ? t('onboarding.steps.getStarted') : t('onboarding.layout.next')
      }
      onNext={handleNext}
      onBack={handleBack}
      onSkip={handleSkip}
    >
      <AnimatePresence mode="wait">
        <motion.div
          key={step}
          variants={slideRight}
          initial="initial"
          animate="animate"
          exit="exit"
          transition={{ duration: 0.2 }}
        >
          {step === 0 && <WelcomeStep />}
          {step === 1 && <DoubaoKeyStep />}
          {step === 2 && <DoneStep />}
        </motion.div>
      </AnimatePresence>
    </OnboardingLayout>
  )
}
