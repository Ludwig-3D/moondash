<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { storeToRefs } from 'pinia'
import { useI18n } from 'vue-i18n'
import { useAppStore } from '@/stores/app'

type AdvancedMetric = {
  label: string
  value: string | null
}

const appStore = useAppStore()
const { locale, t } = useI18n()
const { moonraker } = storeToRefs(appStore)

function parseFiniteNumber(value: unknown): number | null {
  if (typeof value === 'number' && Number.isFinite(value)) return value
  const parsed = Number(value)
  return Number.isFinite(parsed) ? parsed : null
}

const currentLocale = computed(() => locale.value || navigator.language)

const numberFormat = computed(() => new Intl.NumberFormat(currentLocale.value, {
  maximumFractionDigits: 1,
}))

const positionNumberFormat = computed(() => new Intl.NumberFormat(currentLocale.value, {
  minimumFractionDigits: 2,
  maximumFractionDigits: 2,
}))

const filamentNumberFormat = computed(() => new Intl.NumberFormat(currentLocale.value, {
  minimumFractionDigits: 2,
  maximumFractionDigits: 2,
}))

function getGcodeMove() {
  return (moonraker.value as any).gcodeMove ?? (moonraker.value as any).gcode_move ?? {}
}

function getPrintStats() {
  return (moonraker.value as any).printStats ?? (moonraker.value as any).print_stats ?? {}
}

function getToolhead() {
  return (moonraker.value as any).toolhead ?? {}
}

function getMotionReport() {
  return (moonraker.value as any).motionReport ?? (moonraker.value as any).motion_report ?? {}
}

function getRawObjects() {
  return (moonraker.value as any).rawObjects ?? (moonraker.value as any).raw_objects ?? {}
}

function getPosition(): unknown[] | null {
  const gcodeMove = getGcodeMove()
  const toolhead = getToolhead()

  if (Array.isArray(gcodeMove.gcode_position)) return gcodeMove.gcode_position
  if (Array.isArray(toolhead.position)) return toolhead.position

  return null
}

function getFilamentDiameter(): number | null {
  const rawObjects = getRawObjects()
  const configfile = rawObjects.configfile ?? (moonraker.value as any).configfile ?? {}
  const diameter = parseFiniteNumber(configfile.config?.extruder?.filament_diameter)

  return diameter && diameter > 0 ? diameter : null
}

const lastExtruderSample = ref<{ e: number, time: number } | null>(null)
const currentFlow = ref<number | null>(null)

watch(
    () => getPosition()?.[3],
    (value) => {
      const e = parseFiniteNumber(value)
      const now = performance.now()

      if (e === null) {
        lastExtruderSample.value = null
        currentFlow.value = null
        return
      }

      const previous = lastExtruderSample.value
      lastExtruderSample.value = { e, time: now }

      if (!previous) return

      const deltaSeconds = (now - previous.time) / 1000
      const deltaExtrusion = e - previous.e

      if (deltaSeconds <= 0 || deltaExtrusion < 0) return

      const filamentDiameter = getFilamentDiameter()

      if (filamentDiameter === null) {
        currentFlow.value = null
        return
      }

      const filamentRadius = filamentDiameter / 2
      const filamentArea = Math.PI * filamentRadius * filamentRadius
      currentFlow.value = (deltaExtrusion / deltaSeconds) * filamentArea
    },
    { immediate: true },
)

const printSpeed = computed(() => {
  const gcodeMove = getGcodeMove()
  const toolhead = getToolhead()
  const motionReport = getMotionReport()

  const liveVelocity = parseFiniteNumber(motionReport.live_velocity)
  const rawGcodeSpeed = parseFiniteNumber(gcodeMove.speed)
  const speed =
      liveVelocity
      ?? (rawGcodeSpeed === null ? null : rawGcodeSpeed / 60)
      ?? parseFiniteNumber(toolhead.velocity)
      ?? parseFiniteNumber(toolhead.print_time_velocity)

  if (speed === null) return null

  return `${numberFormat.value.format(speed)} mm/s`
})

const activeVelocity = computed(() => {
  const rawObjects = getRawObjects()
  const toolhead = rawObjects.toolhead as Record<string, unknown> | undefined

  const velocity = parseFiniteNumber(toolhead?.max_velocity)

  if (velocity === null) return null

  return `${numberFormat.value.format(velocity)} mm/s`
})

const activeAcceleration = computed(() => {
  const toolhead = getToolhead()
  const motionReport = getMotionReport()
  const rawObjects = getRawObjects()
  const rawToolhead = rawObjects.toolhead as Record<string, unknown> | undefined

  const accel =
      parseFiniteNumber(motionReport.live_accel)
      ?? parseFiniteNumber(motionReport.live_acceleration)
      ?? parseFiniteNumber(toolhead.max_accel)
      ?? parseFiniteNumber(rawToolhead?.max_accel)

  if (accel === null) return null

  return `${numberFormat.value.format(accel)} mm/s²`
})

const printFlow = computed(() => {
  if (currentFlow.value === null) return null
  return `${numberFormat.value.format(currentFlow.value)} mm³/s`
})

const printPosition = computed(() => {
  const position = getPosition()

  if (!position) return null

  const [x, y, z] = position
      .slice(0, 3)
      .map((value: unknown) => parseFiniteNumber(value))

  if (x === null || y === null || z === null) return null

  return [
    `X ${positionNumberFormat.value.format(x)}`,
    `Y ${positionNumberFormat.value.format(y)}`,
    `Z ${positionNumberFormat.value.format(z)}`,
  ].join(' · ')
})

const filamentLength = computed(() => {
  const printStats = getPrintStats()
  const length = parseFiniteNumber(printStats.filamentUsed ?? printStats.filament_used)

  if (length === null) return null

  if (length >= 1000) {
    return `${filamentNumberFormat.value.format(length / 1000)} m`
  }

  return `${filamentNumberFormat.value.format(length)} mm`
})

const advancedMetrics = computed<AdvancedMetric[]>(() => [
  { label: t('advanced_details.speed'), value: printSpeed.value },
  { label: t('advanced_details.flow'), value: printFlow.value },
  { label: t('advanced_details.position'), value: printPosition.value },
  { label: t('advanced_details.velocity'), value: activeVelocity.value },
  { label: t('advanced_details.acceleration'), value: activeAcceleration.value },
  { label: t('advanced_details.filament_length'), value: filamentLength.value },
].filter((metric): metric is AdvancedMetric & { value: string } => metric.value !== null))
</script>

<template>
  <v-card
      class="advanced-details-panel my-2 mr-2"
      rounded="lg"
      variant="flat"
  >
    <div class="advanced-details-panel__items">
      <div
          v-for="metric in advancedMetrics"
          :key="metric.label"
          class="advanced-details-panel__item"
      >
        <div class="advanced-details-panel__label">{{ metric.label }}</div>
        <div class="advanced-details-panel__value">{{ metric.value }}</div>
      </div>
    </div>
  </v-card>
</template>

<style scoped>
.advanced-details-panel {
  width: 260px;
  height: 100%;
  max-height: calc(100vh - 16px);
  padding: 14px;
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.advanced-details-panel__title {
  font-size: 1.05rem;
  font-weight: 700;
}

.advanced-details-panel__items {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.advanced-details-panel__item {
  padding: 10px 12px;
  border-radius: 12px;
  background: rgba(var(--v-theme-on-surface), 0.06);
}

.advanced-details-panel__label {
  font-size: 0.75rem;
  opacity: 0.72;
}

.advanced-details-panel__value {
  margin-top: 2px;
  font-size: 1rem;
  font-weight: 700;
  white-space: nowrap;
}
</style>
