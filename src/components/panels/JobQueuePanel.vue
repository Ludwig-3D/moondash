<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch, nextTick } from 'vue'
import { storeToRefs } from 'pinia'
import { useI18n } from 'vue-i18n'
import { moonraker as moonrakerClient } from '@/plugins/moonraker'
import { useAppStore } from '@/stores/app'

const { t } = useI18n()

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
  count: number
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

const active = computed(() => queuedJobs.value.length > 0)

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

function queuedCountForFilename(filename: string): number {
  const normalized = normalizeMoonrakerFilePath(filename)

  return queuedJobs.value.filter((job) => {
    const queuedFilename = typeof job.filename === 'string' ? job.filename.trim() : ''
    return queuedFilename && normalizeMoonrakerFilePath(queuedFilename) === normalized
  }).length
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
      count: 0,
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
      count: queuedCountForFilename(filename),
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

async function deleteJob(job: PanelJob) {
  if (job.isCurrent) return

  const queuedJob = queuedJobs.value.find((item) => {
    const filename = typeof item.filename === 'string' ? item.filename.trim() : ''
    return normalizeMoonrakerFilePath(filename) === normalizeMoonrakerFilePath(job.filename)
  })

  if (queuedJob?.job_id === undefined || queuedJob.job_id === null) return

  await moonrakerClient.call('server.job_queue.delete_job', { job_ids: [queuedJob.job_id] })
  await refreshPanel()
}

const filenameWrapRefs = new Map<string, HTMLElement>()
const filenameTextRefs = new Map<string, HTMLElement>()
const overflowingFilenames = ref<Record<string, boolean>>({})
let filenameResizeObserver: ResizeObserver | null = null

function setFilenameWrapRef(key: string, el: HTMLElement | null) {
  if (el) {
    filenameWrapRefs.set(key, el)
  } else {
    filenameWrapRefs.delete(key)
  }
}

function setFilenameTextRef(key: string, el: HTMLElement | null) {
  if (el) {
    filenameTextRefs.set(key, el)
  } else {
    filenameTextRefs.delete(key)
  }
}

function isFilenameOverflowing(key: string): boolean {
  return overflowingFilenames.value[key] === true
}

async function updateFilenameOverflow() {
  await nextTick()

  const next: Record<string, boolean> = {}

  for (const job of panelJobs.value) {
    const wrap = filenameWrapRefs.get(job.key)
    const text = filenameTextRefs.get(job.key)

    if (!wrap || !text) {
      next[job.key] = false
      continue
    }

    const firstItem = text.firstElementChild as HTMLElement | null
    const contentWidth = firstItem
        ? Math.ceil(firstItem.getBoundingClientRect().width)
        : Math.ceil(text.getBoundingClientRect().width)

    const visibleWidth = Math.floor(wrap.getBoundingClientRect().width)
    next[job.key] = contentWidth > visibleWidth + 1
  }

  overflowingFilenames.value = next
}

function observeFilenameElements() {
  filenameResizeObserver?.disconnect()

  for (const el of filenameWrapRefs.values()) {
    filenameResizeObserver?.observe(el)
  }

  for (const el of filenameTextRefs.values()) {
    filenameResizeObserver?.observe(el)
  }
}

onMounted(() => {
  refreshPanel()
  refreshTimer = window.setInterval(refreshPanel, 5000)

  filenameResizeObserver = new ResizeObserver(() => {
    void updateFilenameOverflow()
  })

  void nextTick(() => {
    observeFilenameElements()
    void updateFilenameOverflow()
  })
})

onBeforeUnmount(() => {
  if (refreshTimer !== null) {
    window.clearInterval(refreshTimer)
    refreshTimer = null
  }

  filenameResizeObserver?.disconnect()
  filenameResizeObserver = null
})

watch(
    panelJobs,
    async () => {
      await nextTick()
      observeFilenameElements()
      await updateFilenameOverflow()
    },
    { flush: 'post' },
)
</script>

<template>
  <v-card v-if="active" class="job-query-panel mt-2 mr-2">
    <div class="job-query-panel__header">
      <div class="job-query-panel__title">{{ t('job_queue.title') }}</div>
      <v-progress-circular v-if="loading" indeterminate size="18" width="2" />
    </div>

    <div class="job-query-panel__list">
      <v-card
          v-for="job in panelJobs"
          :key="job.key"
          class="job-query-panel__card"
          :class="{ 'job-query-panel__card--current': job.isCurrent }"
          rounded="lg"
          elevation="0"
      >
        <v-card-text class="pa-0">
          <div class="job-query-panel__card-layout">
            <div
                class="job-query-panel__thumb"
                :style="{
                backgroundImage: job.thumbnailUrl ? `url(${job.thumbnailUrl})` : 'none',
              }"
            >
              <div v-if="!job.thumbnailUrl" class="job-query-panel__thumb-placeholder">
                <v-icon icon="mdi-printer-3d" size="42" />
              </div>
            </div>

            <div class="job-query-panel__right pa-0 pt-2">
              <div class="job-query-panel__info px-2">
                <div
                    :ref="(el) => setFilenameWrapRef(job.key, el as HTMLElement | null)"
                    class="job-query-panel__filename-wrap"
                    :title="displayName(job.filename)"
                >
                  <div
                      :ref="(el) => setFilenameTextRef(job.key, el as HTMLElement | null)"
                      class="job-query-panel__filename"
                      :class="{ 'job-query-panel__marquee': isFilenameOverflowing(job.key) }"
                  >
                    <span>{{ displayName(job.filename) }}</span>
                    <span v-if="isFilenameOverflowing(job.key)" aria-hidden="true">
                      {{ displayName(job.filename) }}
                    </span>
                  </div>
                </div>

                <div class="job-query-panel__meta">
                  <span class="job-query-panel__meta-item">
                    <v-icon icon="mdi-clock-outline" size="16" />
                    <span>{{ formatDuration(job.metadata?.estimated_time) }}</span>
                  </span>

                  <span class="job-query-panel__meta-item">
                    <v-icon icon="mdi-scale-balance" size="16" />
                    <span>{{ formatWeight(job.metadata) }}</span>
                  </span>

                  <span v-if="!job.isCurrent && job.count > 1" class="job-query-panel__meta-item">
                    <v-icon icon="mdi-printer" size="18" />
                    <span>×{{ job.count }}</span>
                  </span>
                </div>
              </div>

              <div class="job-query-panel__actions">
                <v-btn
                    class="job-query-panel__actions-btn"
                    size="small"
                    variant="text"
                    :color="job.isCurrent ? undefined : 'primary'"
                    :disabled="job.isCurrent"
                    @click="printJob(job.filename)"
                >
                  <v-icon icon="mdi-play" />
                </v-btn>

                <v-btn
                    class="job-query-panel__actions-btn"
                    size="small"
                    variant="text"
                    color="error"
                    :disabled="job.isCurrent"
                    @click="deleteJob(job)"
                >
                  <v-icon icon="mdi-delete-outline" />
                </v-btn>
              </div>
            </div>
          </div>
        </v-card-text>
      </v-card>
    </div>
  </v-card>
</template>

<style scoped>
.job-query-panel {
  width: 40vw;
  max-width: 40vw !important;
  height: 100%;
  max-height: calc(100vh - 16px);
  display: flex;
  flex-direction: column;
  overflow: hidden;
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

.job-query-panel__card {
  flex: 0 0 auto;
  overflow: hidden;
  background: rgba(var(--v-theme-on-surface), 0.06);
  height: 96px;
}

.job-query-panel__card--current {
  background: rgba(var(--v-theme-primary), 0.16);
  border: 1px solid rgba(var(--v-theme-primary), 0.55);
}

.job-query-panel__card-layout {
  display: grid;
  grid-template-columns: 100px minmax(0, 1fr);
  align-items: stretch;
}

.job-query-panel__thumb {
  width: 96px;
  height: 96px;
  overflow: hidden;
  background-size: cover;
  background-position: center;
  background-repeat: no-repeat;
  display: flex;
  align-items: center;
  justify-content: center;
  background-color: rgba(var(--v-theme-on-surface), 0.08);
}

.job-query-panel__thumb-placeholder {
  display: flex;
  align-items: center;
  justify-content: center;
  opacity: 0.55;
  width: 100%;
  height: 100%;
}

.job-query-panel__right {
  display: flex;
  flex-direction: column;
  justify-content: space-between;
  min-width: 0;
  min-height: 96px;
  gap: 8px;
}

.job-query-panel__info {
  display: flex;
  flex-direction: column;
  min-width: 0;
  gap: 8px;
}

.job-query-panel__filename-wrap {
  min-width: 0;
  width: 100%;
  overflow: hidden;
  white-space: nowrap;
}

.job-query-panel__filename {
  display: inline-flex;
  align-items: center;
  min-width: 100%;
  font-size: 0.95rem;
  font-weight: 700;
  line-height: 1.2;
  white-space: nowrap;
  will-change: transform;
}

.job-query-panel__filename span {
  flex: 0 0 auto;
  padding-right: 40px;
}

.job-query-panel__marquee {
  animation: job-query-panel-marquee 14s linear infinite;
}

.job-query-panel__filename-wrap:hover .job-query-panel__marquee {
  animation-play-state: paused;
}

@keyframes job-query-panel-marquee {
  0% {
    transform: translateX(0);
  }

  8% {
    transform: translateX(0);
  }

  92% {
    transform: translateX(-50%);
  }

  100% {
    transform: translateX(-50%);
  }
}

@media (prefers-reduced-motion: reduce) {
  .job-query-panel__marquee {
    animation: none;
  }
}

.job-query-panel__meta {
  display: flex;
  flex-wrap: wrap;
  gap: 8px 10px;
  opacity: 0.85;
  font-size: 0.84rem;
}

.job-query-panel__meta-item {
  display: inline-flex;
  align-items: center;
  gap: 5px;
}

.job-query-panel__actions {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 4px;
  width: 100%;
}

.job-query-panel__actions-btn {
  flex: 1;
}
</style>