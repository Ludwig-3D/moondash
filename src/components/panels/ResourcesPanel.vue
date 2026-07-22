<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useAppStore } from '@/stores/app'

type AnyRecord = Record<string, any>

type McuObject = {
  app?: string
  mcu_version?: string
  mcu_constants?: {
    CLOCK_FREQ?: number | null
    MCU?: string | null
  }
  last_stats?: {
    mcu_awake?: number | null
    mcu_task_avg?: number | null
    freq?: number | null
  }
  temperature?: number | null
}

const { t } = useI18n()
const appStore = useAppStore()

const moonraker = computed<AnyRecord>(() => appStore.moonraker as AnyRecord)
const procStats = computed<AnyRecord>(() => moonraker.value.procStats ?? {})
const hostResources = computed<AnyRecord>(() => moonraker.value.hostResources ?? {})
const systemInfo = computed<AnyRecord>(() =>
    moonraker.value.systemInfo ?? hostResources.value.systemInfo ?? {},
)
const fullProcStats = computed<AnyRecord>(() => ({
  ...(moonraker.value.fullProcStats ?? {}),
  ...(hostResources.value.procStats ?? {}),
  ...procStats.value,
}))

const cpuUsage = computed(() => normalizePercent(firstNumber(
    procStats.value.moonrakerCpuUsage,
    procStats.value.moonraker_cpu_usage,
    fullProcStats.value.moonraker_stats?.cpu_usage,
    fullProcStats.value.moonrakerStats?.cpuUsage,
)))

const cpuTemperature = computed(() => firstNumber(
    procStats.value.cpuTemp,
    procStats.value.cpu_temp,
    fullProcStats.value.cpu_temp,
    systemInfo.value.cpu_info?.temperature,
))

const hostLoad = computed(() => {
  const systemCpu = firstValue(
      procStats.value.systemCpuUsage,
      procStats.value.system_cpu_usage,
      fullProcStats.value.systemCpuUsage,
      fullProcStats.value.system_cpu_usage,
  )

  if (systemCpu && typeof systemCpu === 'object') {
    return firstNumber(
        systemCpu.cpu,
        systemCpu.total,
        systemCpu.usage,
        systemCpu.value,
    )
  }

  return firstNumber(systemCpu)
})

const systemMemory = computed<AnyRecord>(() =>
    procStats.value.system_memory ??
    procStats.value.systemMemory ??
    fullProcStats.value.system_memory ??
    fullProcStats.value.systemMemory ??
    {},
)

const memoryUnit = computed(() => String(
    systemMemory.value.units ??
    systemMemory.value.unit ??
    systemMemory.value.mem_units ??
    'kb',
).toLowerCase())

const totalMemoryBytes = computed(() => {
  const value = firstNumber(
      systemMemory.value.total,
      systemMemory.value.total_memory,
      systemMemory.value.mem_total,
      hostResources.value.totalMemory,
      systemInfo.value.cpu_info?.total_memory,
  )

  return value === null ? null : memoryValueToBytes(value, memoryUnit.value)
})

const usedMemoryBytes = computed(() => {
  const explicitUsed = firstNumber(
      systemMemory.value.used,
      systemMemory.value.used_memory,
      systemMemory.value.mem_used,
  )

  if (explicitUsed !== null) {
    return memoryValueToBytes(explicitUsed, memoryUnit.value)
  }

  const available = firstNumber(
      systemMemory.value.available,
      systemMemory.value.available_memory,
      systemMemory.value.mem_available,
  )

  if (available !== null && totalMemoryBytes.value !== null) {
    return Math.max(
        0,
        totalMemoryBytes.value - memoryValueToBytes(available, memoryUnit.value),
    )
  }

  const free = firstNumber(systemMemory.value.free, systemMemory.value.mem_free)
  const buffers = firstNumber(systemMemory.value.buffers) ?? 0
  const cached = firstNumber(systemMemory.value.cached) ?? 0

  if (free !== null && totalMemoryBytes.value !== null) {
    return Math.max(
        0,
        totalMemoryBytes.value -
        memoryValueToBytes(free + buffers + cached, memoryUnit.value),
    )
  }

  return null
})

const memoryUsage = computed(() => {
  const explicitPercent = firstNumber(
      systemMemory.value.percent,
      systemMemory.value.usage,
      systemMemory.value.used_percent,
  )

  if (explicitPercent !== null) {
    return normalizePercent(explicitPercent)
  }

  if (
      usedMemoryBytes.value === null ||
      totalMemoryBytes.value === null ||
      totalMemoryBytes.value <= 0
  ) {
    return null
  }

  return normalizePercent((usedMemoryBytes.value / totalMemoryBytes.value) * 100)
})

const storage = computed<AnyRecord>(() =>
    moonraker.value.storage ??
    hostResources.value.storage ??
    {},
)

const totalStorageBytes = computed(() => firstNumber(storage.value.total))
const usedStorageBytes = computed(() => firstNumber(storage.value.used))

const storageUsage = computed(() => {
  if (
      usedStorageBytes.value === null ||
      totalStorageBytes.value === null ||
      totalStorageBytes.value <= 0
  ) {
    return null
  }

  return normalizePercent((usedStorageBytes.value / totalStorageBytes.value) * 100)
})

const mcus = computed(() => {
  const rawObjects = moonraker.value.rawObjects ?? {}
  const config = rawObjects.configfile?.config ?? {}

  const mcuTemperatures = new Map<string, number>()

  for (const [key, raw] of Object.entries(rawObjects)) {
    if (
        !key.startsWith('temperature_sensor ') &&
        !key.startsWith('temperature_fan ')
    ) {
      continue
    }

    const sensorConfig = config[key] ?? {}
    if (sensorConfig.sensor_type !== 'temperature_mcu') continue

    const mcuName = String(sensorConfig.sensor_mcu ?? 'mcu')
    const temperature = firstNumber((raw as AnyRecord).temperature)

    if (temperature !== null) {
      mcuTemperatures.set(mcuName, temperature)
    }
  }

  return Object.entries(rawObjects)
      .filter(([key]) => key === 'mcu' || key.startsWith('mcu '))
      .map(([key, raw]) => {
        const mcu = raw as McuObject
        const mcuName = key === 'mcu' ? 'mcu' : key.slice(4)
        const awake = firstNumber(mcu.last_stats?.mcu_awake)
        const taskAverage = firstNumber(mcu.last_stats?.mcu_task_avg)
        const measuredFrequency = firstNumber(mcu.last_stats?.freq)
        const configuredFrequency = firstNumber(mcu.mcu_constants?.CLOCK_FREQ)

        return {
          key,
          name: key === 'mcu' ? t('settings.resources.main_mcu') : mcuName,
          version: [mcu.app, mcu.mcu_version].filter(Boolean).join(' ') || '—',
          load: taskAverage === null ? null : taskAverage * 1000,
          awake,
          usage: awake === null ? null : normalizePercent(awake * 100),
          frequency: measuredFrequency ?? configuredFrequency,
          chipset: mcu.mcu_constants?.MCU || '—',
          temperature: firstNumber(
              mcu.temperature,
              mcuTemperatures.get(mcuName),
          ),
        }
      })
      .sort((a, b) => {
        if (a.key === 'mcu') return -1
        if (b.key === 'mcu') return 1
        return a.name.localeCompare(b.name)
      })
})

function firstValue(...values: unknown[]): unknown {
  for (const value of values) {
    if (value !== null && value !== undefined) return value
  }
  return null
}

function firstNumber(...values: unknown[]): number | null {
  for (const value of values) {
    if (typeof value === 'number' && Number.isFinite(value)) return value
  }
  return null
}

function normalizePercent(value: number | null): number | null {
  if (value === null) return null
  return Math.max(0, Math.min(100, value))
}

function memoryValueToBytes(value: number, unit: string): number {
  if (unit.includes('g')) return value * 1024 ** 3
  if (unit.includes('m')) return value * 1024 ** 2
  if (unit.includes('k')) return value * 1024
  return value
}

function formatPercent(value: number | null): string {
  return value === null ? '—' : `${value.toFixed(1)} %`
}

function usageClass(value: number | null): string | undefined {
  if (value === null) return undefined
  if (value >= 90) return 'text-red'
  if (value >= 80) return 'text-orange'
  return undefined
}

function usageColor(value: number | null): string | undefined {
  if (value === null) return undefined
  if (value >= 90) return 'red'
  if (value >= 80) return 'orange'
  return undefined
}

function temperatureClass(value: number | null): string | undefined {
  if (value === null) return undefined
  if (value >= 90) return 'text-red'
  if (value >= 80) return 'text-orange'
  return undefined
}

function formatTemperature(value: number | null): string {
  return value === null ? '—' : `${Math.round(value)} °C`
}

function formatBytes(value: number | null): string {
  if (value === null) return '—'

  const units = ['B', 'kB', 'MB', 'GB', 'TB']
  let size = value
  let unitIndex = 0

  while (size >= 1024 && unitIndex < units.length - 1) {
    size /= 1024
    unitIndex++
  }

  const digits = unitIndex >= 3 || size < 10 ? 1 : 0
  return `${size.toFixed(digits)} ${units[unitIndex]}`
}

function formatFrequency(value: number | null): string {
  return value === null ? '—' : `${Math.round(value / 1_000_000)} MHz`
}

function formatDecimal(value: number | null): string {
  return value === null ? '—' : value.toFixed(2)
}
</script>

<template>
  <v-row density="comfortable" align="start" class="resources-row">
    <v-col cols="12" sm="6" class="resource-col host-col">
      <v-card rounded="lg" variant="tonal">
        <v-card-text class="host-content pa-3">
          <section class="metric-section">
            <div class="metric-header">
              <div class="metric-label">
                <v-icon icon="mdi-cpu-64-bit" size="19" />
                <span>{{ t('settings.resources.cpu_usage') }}</span>
              </div>
              <strong :class="usageClass(cpuUsage)">
                {{ formatPercent(cpuUsage) }}
              </strong>
            </div>

            <v-progress-linear
                :model-value="cpuUsage ?? 0"
                :color="usageColor(cpuUsage)"
                rounded
                height="8"
            />

            <v-row density="comfortable" class="metric-details">
              <v-col cols="6" class="py-0">
                {{ t('settings.resources.load') }}:
                <strong>{{ formatDecimal(hostLoad) }}</strong>
              </v-col>

              <v-col cols="6" class="py-0 text-right">
                {{ t('settings.resources.temperature') }}:
                <strong :class="temperatureClass(cpuTemperature)">
                  {{ formatTemperature(cpuTemperature) }}
                </strong>
              </v-col>
            </v-row>
          </section>

          <v-divider />

          <section class="metric-section">
            <div class="metric-header">
              <div class="metric-label">
                <v-icon icon="mdi-memory" size="19" />
                <span>{{ t('settings.resources.ram_usage') }}</span>
              </div>
              <strong :class="usageClass(memoryUsage)">
                {{ formatPercent(memoryUsage) }}
              </strong>
            </div>

            <v-progress-linear
                :model-value="memoryUsage ?? 0"
                :color="usageColor(memoryUsage)"
                rounded
                height="8"
            />

            <div class="text-caption">
              <strong>{{ formatBytes(usedMemoryBytes) }}</strong>
              /
              {{ formatBytes(totalMemoryBytes) }}
            </div>
          </section>

          <v-divider />

          <section class="metric-section">
            <div class="metric-header">
              <div class="metric-label">
                <v-icon icon="mdi-harddisk" size="19" />
                <span>{{ t('settings.resources.storage_usage') }}</span>
              </div>
              <strong :class="usageClass(storageUsage)">
                {{ formatPercent(storageUsage) }}
              </strong>
            </div>

            <v-progress-linear
                :model-value="storageUsage ?? 0"
                :color="usageColor(storageUsage)"
                rounded
                height="8"
            />

            <div class="text-caption">
              <strong>{{ formatBytes(usedStorageBytes) }}</strong>
              /
              {{ formatBytes(totalStorageBytes) }}
            </div>
          </section>
        </v-card-text>
      </v-card>
    </v-col>

    <v-col cols="12" sm="6" class="resource-col mcu-col">
      <v-card rounded="lg" variant="tonal" class="mcu-card">
        <v-card-text class="mcu-list pa-0">
          <template v-for="(mcu, index) in mcus" :key="mcu.key">
            <section class="mcu-entry pa-3">
              <div class="mcu-header">
                <div class="mcu-name">
                  <v-icon icon="mdi-chip" size="18" />
                  <strong>{{ mcu.name }}</strong>
                </div>

                <strong :class="usageClass(mcu.usage)">
                  {{ formatPercent(mcu.usage) }}
                </strong>
              </div>

              <v-progress-linear
                  :model-value="mcu.usage ?? 0"
                  :color="usageColor(mcu.usage)"
                  rounded
                  height="7"
              />

              <v-row density="comfortable" class="mcu-details" no-gutters>
                <v-col cols="12" class="" v-if="mcu.temperature">
                  <div class="detail-pair">
                    <span>{{ t('settings.resources.temperature') }}</span>
                    <strong :class="temperatureClass(mcu.temperature)">
                      {{ formatTemperature(mcu.temperature) }} {{}}
                    </strong>
                  </div>
                </v-col>

                <v-col cols="12" class="">
                  <div class="detail-pair">
                    <span>{{ t('settings.resources.version') }}</span>
                    <strong>{{ mcu.version }}</strong>
                  </div>
                </v-col>

                <v-col cols="12" class="">
                  <div class="detail-pair">
                    <span>{{ t('settings.resources.chipset') }}</span>
                    <strong>{{ mcu.chipset }}</strong>
                  </div>
                </v-col>

                <v-col cols="12" class="">
                  <div class="detail-pair">
                    <span>{{ t('settings.resources.frequency') }}</span>
                    <strong>{{ formatFrequency(mcu.frequency) }}</strong>
                  </div>
                </v-col>
              </v-row>
            </section>

            <v-divider v-if="index < mcus.length - 1" />
          </template>

          <div v-if="mcus.length === 0" class="pa-3">
            <v-alert type="info" variant="tonal" density="compact">
              {{ t('settings.resources.no_mcus') }}
            </v-alert>
          </div>
        </v-card-text>
      </v-card>
    </v-col>
  </v-row>
</template>

<style scoped>
.resources-row {
  align-items: flex-start;
}

.resource-col {
  padding-top: 4px !important;
  padding-bottom: 4px !important;
}

.resource-col > .v-card {
  width: 100%;
}

.host-col {
  align-self: flex-start;
}

.mcu-col {
  align-self: flex-start;
}

.host-content {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.metric-section {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.metric-header,
.metric-label,
.mcu-header,
.mcu-name,
.detail-pair {
  display: flex;
  align-items: center;
}

.metric-header,
.mcu-header,
.detail-pair {
  justify-content: space-between;
  gap: 10px;
}

.metric-label,
.mcu-name {
  gap: 7px;
}

.metric-details,
.mcu-details,
.detail-pair,
.metric-value {
  font-size: 0.82rem;
}

.mcu-card {
  overflow: hidden;
}

.mcu-list {
  max-height: calc(100vh - 73px);
  overflow-y: auto;
  overflow-x: hidden;
}

.mcu-entry {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.detail-pair {
  align-items: flex-start;
}

.detail-pair span,
.metric-value span {
  opacity: 0.72;
}

.detail-pair strong {
  min-width: 0;
  text-align: right;
  overflow-wrap: anywhere;
}

.metric-value {
  display: flex;
  flex-direction: column;
  gap: 1px;
}
</style>
