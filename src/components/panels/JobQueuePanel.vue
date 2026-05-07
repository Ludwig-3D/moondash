<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { storeToRefs } from 'pinia'
import { moonraker as moonrakerClient } from '@/plugins/moonraker'
import { useAppStore } from '@/stores/app'

const emit = defineEmits<{
  (e: 'active-change', value: boolean): void
}>()

const appStore = useAppStore()
const { moonraker } = storeToRefs(appStore)

type QueueJob = {
  job_id?: string | number
  filename?: string
  time_added?: number
  time_in_queue?: number
  [key: string]: unknown
}

type JobQueueStatus = {
  queued_jobs?: QueueJob[]
  queue_state?: string
}

type JobMetadata = {
  estimated_time?: number
  filament_weight_total?: number
  filament_weight?: number
  filament?: {
    weight_total?: number
    weight?: number
  }
  thumbnails?: Array<{
    thumbnail_path?: string
    width?: number
    height?: number
  }>
}

type PanelJob = {
  key: string
  filename: string
  isCurrent: boolean
  metadata: JobMetadata | null
  thumbnailUrl: string | null
}

const queuedJobs = ref<QueueJob[]>([])
const queueState = ref('')
const metadataByFilename = ref<Record<string, JobMetadata | null>>({})
const thumbnailsByFilename = ref<Record<string, string | null>>({})
const loading = ref(false)
let refreshTimer: number | null = null

const currentFilename = computed(() => {
  const filename = moonraker.value.printStats?.filename
  return typeof filename === 'string' && filename.trim() ? filename.trim() : ''
})


const hasQueue = computed(() => queuedJobs.value.length > 0)
const active = computed(() => {
  const state = queueState.value.toLowerCase()
  return hasQueue.value || ['loading', 'paused'].includes(state)
})

watch(active, (value) => emit('active-change', value), { immediate: true })

function normalizeMoonrakerFilePath(path: string): string {
  return path
    .trim()
    .replace(/^\/+/, '')
    .replace(/^gcodes\//, '')
    .replace(/^\.\//, '')
}

function encodeMoonrakerFilePath(path: string): string {
  return normalizeMoonrakerFilePath(path)
    .split('/')
    .map((segment) => encodeURIComponent(segment))
    .join('/')
}

function getMoonrakerHttpBase(): string | null {
  const wsUrl = moonrakerClient.getStatus().url
  if (!wsUrl) return null

  try {
    const parsed = new URL(wsUrl)
    const protocol = parsed.protocol === 'wss:' ? 'https:' : 'http:'
    return `${protocol}//${parsed.host}`
  } catch {
    return null
  }
}

function resolveThumbnailPath(filePath: string, thumbnailPath: string): string {
  const normalizedFilePath = normalizeMoonrakerFilePath(filePath)
  const normalizedThumbPath = normalizeMoonrakerFilePath(thumbnailPath)
  const fileDir = normalizedFilePath.includes('/')
    ? normalizedFilePath.slice(0, normalizedFilePath.lastIndexOf('/'))
    : ''

  if (!fileDir) return normalizedThumbPath

  if (
    normalizedThumbPath.startsWith(`${fileDir}/`) ||
    normalizedThumbPath.startsWith(`.thumbs/${fileDir}/`)
  ) {
    return normalizedThumbPath
  }

  return `${fileDir}/${normalizedThumbPath}`
}

function thumbnailUrlFromPath(filename: string, thumbnailPath: string): string | null {
  const base = getMoonrakerHttpBase()
  if (!base) return null

  const fullThumbPath = resolveThumbnailPath(filename, thumbnailPath)
  return `${base}/server/files/gcodes/${encodeMoonrakerFilePath(fullThumbPath)}`
}

function formatDuration(seconds: unknown): string {
  const value = typeof seconds === 'number' ? seconds : Number(seconds)
  if (!Number.isFinite(value) || value < 0) return '--'

  let remaining = Math.floor(value)
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

function formatWeight(metadata: JobMetadata | null): string {
  const weight =
    metadata?.filament_weight_total ??
    metadata?.filament_weight ??
    metadata?.filament?.weight_total ??
    metadata?.filament?.weight

  if (typeof weight !== 'number' || !Number.isFinite(weight)) return '--'
  return `${weight.toFixed(1)}g`
}

function displayName(filename: string): string {
  const name = filename.split('/').pop() || filename
  return name.replace(/\.gcode$/i, '')
}

const panelJobs = computed<PanelJob[]>(() => {
  const seen = new Set<string>()
  const jobs: PanelJob[] = []

  if (currentFilename.value) {
    seen.add(normalizeMoonrakerFilePath(currentFilename.value))
    jobs.push({
      key: `current:${currentFilename.value}`,
      filename: currentFilename.value,
      isCurrent: true,
      metadata: metadataByFilename.value[currentFilename.value] ?? null,
      thumbnailUrl: thumbnailsByFilename.value[currentFilename.value] ?? null,
    })
  }

  for (const job of queuedJobs.value) {
    const filename = typeof job.filename === 'string' ? job.filename.trim() : ''
    if (!filename) continue

    const normalized = normalizeMoonrakerFilePath(filename)
    if (seen.has(normalized)) continue
    seen.add(normalized)

    jobs.push({
      key: `queue:${job.job_id ?? filename}`,
      filename,
      isCurrent: false,
      metadata: metadataByFilename.value[filename] ?? null,
      thumbnailUrl: thumbnailsByFilename.value[filename] ?? null,
    })
  }

  return jobs
})

async function fetchJobQueue() {
  try {
    const result = await moonrakerClient.call<JobQueueStatus>('server.job_queue.status')
    queuedJobs.value = Array.isArray(result?.queued_jobs) ? result.queued_jobs : []
    queueState.value = typeof result?.queue_state === 'string' ? result.queue_state : ''
  } catch {
    queuedJobs.value = []
    queueState.value = ''
  }
}

async function fetchMetadata(filename: string) {
  if (Object.prototype.hasOwnProperty.call(metadataByFilename.value, filename)) return

  try {
    const result = await moonrakerClient.call<JobMetadata>('server.files.metadata', { filename })
    metadataByFilename.value = {
      ...metadataByFilename.value,
      [filename]: result ?? null,
    }
  } catch {
    metadataByFilename.value = {
      ...metadataByFilename.value,
      [filename]: null,
    }
  }
}

async function fetchThumbnail(filename: string) {
  if (Object.prototype.hasOwnProperty.call(thumbnailsByFilename.value, filename)) return

  try {
    const result = await moonrakerClient.call<unknown>('server.files.thumbnails', { filename })
    let thumbnails: JobMetadata['thumbnails'] = []

    if (Array.isArray(result)) {
      thumbnails = result as JobMetadata['thumbnails']
    } else if (result && typeof result === 'object') {
      const record = result as Record<string, unknown>
      if (Array.isArray(record.thumbnails)) thumbnails = record.thumbnails as JobMetadata['thumbnails']
      if (Array.isArray(record.result)) thumbnails = record.result as JobMetadata['thumbnails']
    }

    const selected = [...(thumbnails ?? [])]
      .sort((a, b) => ((b.width ?? 0) * (b.height ?? 0)) - ((a.width ?? 0) * (a.height ?? 0)))
      .find((item) => item.thumbnail_path)

    thumbnailsByFilename.value = {
      ...thumbnailsByFilename.value,
      [filename]: selected?.thumbnail_path ? thumbnailUrlFromPath(filename, selected.thumbnail_path) : null,
    }
  } catch {
    thumbnailsByFilename.value = {
      ...thumbnailsByFilename.value,
      [filename]: null,
    }
  }
}

async function refreshPanel() {
  loading.value = true

  try {
    await fetchJobQueue()

    const filenames = panelJobs.value.map((job) => job.filename)
    await Promise.all(filenames.map((filename) => Promise.all([
      fetchMetadata(filename),
      fetchThumbnail(filename),
    ])))
  } finally {
    loading.value = false
  }
}

async function printJob(filename: string) {
  await moonrakerClient.call('printer.print.start', { filename })
}

onMounted(() => {
  refreshPanel()
  refreshTimer = window.setInterval(refreshPanel, 5000)
})

onBeforeUnmount(() => {
  if (refreshTimer !== null) {
    window.clearInterval(refreshTimer)
    refreshTimer = null
  }
})
</script>

<template>
  <v-card v-if="active" class="job-query-panel" rounded="lg">
    <div class="job-query-panel__header">
      <div class="job-query-panel__title">Job queue</div>
      <v-progress-circular v-if="loading" indeterminate size="18" width="2" />
    </div>

    <div class="job-query-panel__list">
      <div
        v-for="job in panelJobs"
        :key="job.key"
        class="job-query-panel__item"
        :class="{ 'job-query-panel__item--current': job.isCurrent }"
      >
        <div
          class="job-query-panel__preview"
          :style="{ backgroundImage: job.thumbnailUrl ? `url(${job.thumbnailUrl})` : 'none' }"
        >
          <v-icon v-if="!job.thumbnailUrl" icon="mdi-printer-3d" size="30" />
        </div>

        <div class="job-query-panel__content">
          <div class="job-query-panel__top-row">
            <div class="job-query-panel__filename">
              {{ displayName(job.filename) }}
            </div>

            <v-btn
              icon="mdi-printer-play"
              size="small"
              variant="text"
              :color="job.isCurrent ? undefined : 'primary'"
              :disabled="job.isCurrent"
              @click="printJob(job.filename)"
            />
          </div>

          <div class="job-query-panel__stats">
            <span>
              <v-icon icon="mdi-scale-balance" size="15" />
              {{ formatWeight(job.metadata) }}
            </span>
            <span>
              <v-icon icon="mdi-clock-outline" size="15" />
              {{ formatDuration(job.metadata?.estimated_time) }}
            </span>
          </div>
        </div>
      </div>
    </div>
  </v-card>
</template>

<style scoped>
.job-query-panel {
  width: 360px;
  max-width: 360px;
  height: 100%;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  background: rgba(var(--v-theme-surface), 1);
}

.job-query-panel__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 12px 14px 8px;
  flex: 0 0 auto;
}

.job-query-panel__title {
  font-weight: 800;
  font-size: 1rem;
}

.job-query-panel__list {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 8px 10px 12px;
  overflow-y: auto;
  min-height: 0;
}

.job-query-panel__item {
  display: flex;
  gap: 10px;
  padding: 8px;
  border-radius: 14px;
  background: rgba(var(--v-theme-on-surface), 0.06);
  min-width: 0;
}

.job-query-panel__item--current {
  background: rgba(var(--v-theme-primary), 0.16);
  border: 1px solid rgba(var(--v-theme-primary), 0.55);
}

.job-query-panel__preview {
  width: 64px;
  height: 64px;
  flex: 0 0 64px;
  border-radius: 12px;
  background-size: cover;
  background-position: center;
  background-repeat: no-repeat;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  opacity: 0.95;
  background-color: rgba(var(--v-theme-on-surface), 0.08);
}

.job-query-panel__content {
  flex: 1 1 auto;
  min-width: 0;
  display: flex;
  flex-direction: column;
  justify-content: space-between;
  gap: 8px;
}

.job-query-panel__top-row {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 8px;
  min-width: 0;
}

.job-query-panel__filename {
  min-width: 0;
  font-weight: 700;
  line-height: 1.2;
  overflow: hidden;
  text-overflow: ellipsis;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
}

.job-query-panel__stats {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 10px;
  opacity: 0.82;
  font-size: 0.85rem;
}

.job-query-panel__stats span {
  display: inline-flex;
  align-items: center;
  gap: 4px;
}
</style>
