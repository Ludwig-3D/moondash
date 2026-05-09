<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { moonraker as moonrakerClient } from '@/plugins/moonraker'


type MoonrakerHistoryJob = {
  filename?: string
  status?: string
  start_time?: number
  end_time?: number | null
}

type MoonrakerHistoryListResult = {
  jobs?: MoonrakerHistoryJob[]
}

const HISTORY_CACHE_TTL_MS = 30_000
let cachedHistoryJobs: MoonrakerHistoryJob[] = []
let historyCacheTimestamp = 0
let historyRequest: Promise<MoonrakerHistoryJob[]> | null = null

async function getHistoryJobs(): Promise<MoonrakerHistoryJob[]> {
  const now = Date.now()
  if (cachedHistoryJobs.length > 0 && now - historyCacheTimestamp < HISTORY_CACHE_TTL_MS) {
    return cachedHistoryJobs
  }

  if (historyRequest) return historyRequest

  historyRequest = moonrakerClient
      .call<MoonrakerHistoryListResult>('server.history.list', {
        limit: 100,
        order: 'desc',
      })
      .then((result) => {
        cachedHistoryJobs = Array.isArray(result?.jobs) ? result.jobs : []
        historyCacheTimestamp = Date.now()
        return cachedHistoryJobs
      })
      .catch((error) => {
        console.warn('Failed to load Moonraker print history:', error)
        return []
      })
      .finally(() => {
        historyRequest = null
      })

  return historyRequest
}

type MoonrakerFile = {
  path?: string
  filename?: string
  display?: string
  modified?: number
  permissions?: string
}

type MoonrakerGcodeMetadata = {
  estimated_time?: number
  filament_weight_total?: number
  filament_weight?: number
  filament?: {
    weight_total?: number
    weight?: number
  }
}

const props = defineProps<{
  file: MoonrakerFile
  title: string
  thumbnailUrl?: string | null
  metadata?: MoonrakerGcodeMetadata | null
  loading?: boolean
  disabled?: boolean
}>()

const emit = defineEmits<{
  (e: 'select', file: MoonrakerFile): void
}>()

function formatPrintTime(seconds: number | undefined): string {
  if (typeof seconds !== 'number' || !Number.isFinite(seconds) || seconds < 0) return ''

  let remaining = Math.floor(seconds)
  const days = Math.floor(remaining / 86400)
  remaining %= 86400
  const hours = Math.floor(remaining / 3600)
  remaining %= 3600
  const minutes = Math.floor(remaining / 60)

  const parts: string[] = []
  if (days > 0) parts.push(`${days}d`)
  if (hours > 0) parts.push(`${hours}h`)
  if (minutes > 0) parts.push(`${minutes}m`)

  return parts.join('') || '0m'
}

const lastPrintStatus = ref<string>('')
const lastPrintCount = ref(0)
let disposed = false

const normalizedFileCandidates = computed(() => {
  const values = [props.file.path, props.file.filename, props.file.display, props.title]
  return values
      .map((value) => normalizeHistoryFilename(value))
      .filter((value): value is string => Boolean(value))
})

const lastPrintStateColor = computed(() => {
  switch (lastPrintStatus.value) {
    case 'completed':
      return 'success'
    case 'failed':
    case 'error':
      return 'error'
    case 'cancelled':
    case 'klippy_shutdown':
      return 'warning'
    default:
      return undefined
  }
})

const lastPrintStateIcon = computed(() => {
  switch (lastPrintStateColor.value) {
    case 'success':
      return 'mdi-check-circle-outline'
    case 'error':
      return 'mdi-close-circle-outline'
    case 'warning':
      return 'mdi-alert-outline'
    default:
      return 'mdi-history'
  }
})

const lastPrintStateLabel = computed(() => {
  if (!lastPrintStatus.value) return ''
  return lastPrintStatus.value
      .split('_')
      .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
      .join(' ')
})

function normalizeHistoryFilename(value: string | undefined | null): string {
  if (!value) return ''

  return value
      .replace(/\\/g, '/')
      .replace(/^.*\/gcodes\//, '')
      .replace(/^\/+/, '')
      .trim()
}

function fileBasename(value: string): string {
  return value.split('/').filter(Boolean).pop() ?? value
}

function findMatchingHistoryJobs(jobs: MoonrakerHistoryJob[]): MoonrakerHistoryJob[] {
  const candidates = normalizedFileCandidates.value
  if (candidates.length === 0) return []

  const exactMatches = jobs.filter((job) => {
    const historyFilename = normalizeHistoryFilename(job.filename)
    return Boolean(historyFilename && candidates.includes(historyFilename))
  })
  if (exactMatches.length > 0) return exactMatches

  const basenames = new Set(candidates.map(fileBasename))
  return jobs.filter((job) => {
    const historyFilename = normalizeHistoryFilename(job.filename)
    return Boolean(historyFilename && basenames.has(fileBasename(historyFilename)))
  })
}

async function loadLastPrintStatus() {
  const jobs = await getHistoryJobs()
  if (disposed) return

  const matchingJobs = findMatchingHistoryJobs(jobs)
  const latestJob = matchingJobs[0]
  lastPrintStatus.value = typeof latestJob?.status === 'string' ? latestJob.status : ''
  lastPrintCount.value = matchingJobs.length
}

onMounted(() => {
  void loadLastPrintStatus()
})

onBeforeUnmount(() => {
  disposed = true
})

watch(
    () => [props.file.path, props.file.filename, props.file.display, props.title],
    () => {
      void loadLastPrintStatus()
    },
)

function selectFile() {
  emit('select', props.file)
}
</script>

<template>
  <v-card
      class="print-file-card"
      :class="{ 'print-file-card--disabled': disabled }"
      variant="flat"
      @click="selectFile"
  >
    <div class="print-file-card__title">
      {{ title }}
    </div>

    <div class="print-file-card__thumb-wrap">
      <img
          v-if="thumbnailUrl"
          :src="thumbnailUrl"
          :alt="title"
          class="print-file-card__thumb"
      >

      <div v-else class="print-file-card__thumb-placeholder">
        <v-progress-circular
            v-if="loading"
            indeterminate
            size="32"
        />
        <v-icon
            v-else
            icon="mdi-printer-3d"
            size="48"
        />
      </div>
    </div>

    <v-card-text class="print-file-card__content">
      <div class="print-file-card__meta">
        <span
            v-if="formatPrintTime(metadata?.estimated_time)"
            class="print-file-card__meta-item print-file-card__meta-item--time"
        >
          <v-icon icon="mdi-clock-outline" size="14" />
          <span>{{ formatPrintTime(metadata?.estimated_time) }}</span>
        </span>

        <span v-else />

        <span
            v-if="lastPrintStatus"
            class="print-file-card__meta-item print-file-card__meta-item--last-state"
            :style="lastPrintStateColor ? { color: `rgb(var(--v-theme-${lastPrintStateColor}))` } : undefined"
            :title="`Last print: ${lastPrintStateLabel} (${lastPrintCount})`"
        >
          <v-icon :icon="lastPrintStateIcon" :color="lastPrintStateColor" size="16" />
          <span>{{ lastPrintCount }}</span>
        </span>
      </div>
    </v-card-text>
  </v-card>
</template>

<style scoped>
.print-file-card {
  overflow: hidden;
  background: transparent !important;
  box-shadow: none !important;
  position: relative;
  display: flex;
  flex-direction: column;
  cursor: pointer;
}

.print-file-card--disabled {
  cursor: default;
  pointer-events: auto;
  opacity: 0.55;
}

.print-file-card__title,
.print-file-card__meta {
  z-index: 2;
  background: rgba(var(--v-theme-background), 0.8);
}

.print-file-card__title {
  font-weight: 600;
  line-height: 1.2;
  word-break: break-word;
  padding: 8px 10px;
  border-radius: 8px 8px 0 0;
  font-size: 0.8rem;
}

.print-file-card__thumb-wrap {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  aspect-ratio: 1.3 / 1;
  background: rgba(var(--v-theme-on-surface), 0.06);
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
}

.print-file-card__thumb {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.print-file-card__thumb-placeholder {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
}

.print-file-card__content {
  padding: 0;
}

.print-file-card__meta {
  position: absolute;
  left: 0;
  bottom: 0;
  width: 100%;
  font-size: 0.9rem;
  opacity: 0.75;
  display: flex;
  justify-content: space-between;
  gap: 10px;
  align-items: center;
  padding: 8px 10px;
  border-radius: 0 0 8px 8px;
}

.print-file-card__meta-item {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  min-width: 0;
}

.print-file-card__meta-item--time {
  margin-right: auto;
}

.print-file-card__meta-item--last-state {
  gap: 3px;
  margin-left: auto;
}

.print-file-card__meta-item--last-state span {
  font-variant-numeric: tabular-nums;
  line-height: 1;
}
</style>
