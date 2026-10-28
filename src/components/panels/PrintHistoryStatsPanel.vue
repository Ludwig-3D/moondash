
<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useAppStore } from '@/stores/app'
import { moonraker } from '@/plugins/moonraker'

type Job = { status?: string }
type HistoryList = { jobs?: Job[]; count?: number }
type HistoryTotals = { job_totals?: Record<string, number> }

const { t } = useI18n()
const store = useAppStore()
const totals = ref<Record<string, number>>({})
const counts = ref<Record<string, number>>({})
const loading = ref(false)
const failed = ref(false)
const view = ref<'chart' | 'details'>('chart')

const colors: Record<string, string> = {
  completed: '#66bb6a',
  cancelled: '#ffb74d',
  error: '#ef5350',
  klippy_shutdown: '#ab7bd6',
  interrupted: '#42a5f5',
  in_progress: '#29b6f6',
}

const connected = computed(() => store.isWebsocketConnected)

const countEntries = computed(() =>
    Object.entries(counts.value)
        .filter(([, n]) => n > 0)
        .sort((a, b) => b[1] - a[1])
)

const countedJobs = computed(() =>
    countEntries.value.reduce((sum, [, n]) => sum + n, 0)
)

const totalJobs = computed(() =>
    totals.value.total_jobs ?? countedJobs.value
)

const averagePrintTime = computed(() => {
  const total = totals.value.total_print_time
  const count = totalJobs.value

  return typeof total === 'number' && count > 0
      ? total / count
      : undefined
})

const chartSegments = computed(() => {
  let offset = 0

  return countEntries.value.map(([key, count]) => {
    const fraction = countedJobs.value
        ? count / countedJobs.value
        : 0

    const start = offset
    offset += fraction

    return {
      key,
      count,
      color: colors[key] ?? '#90a4ae',
      dash: `${fraction * 100} ${100 - fraction * 100}`,
      offset: -start * 100,
    }
  })
})

function formatDuration(raw?: number): string {
  if (raw == null || !Number.isFinite(raw)) return '—'

  const secs = Math.max(0, Math.floor(raw))
  const h = Math.floor(secs / 3600)
  const m = Math.floor((secs % 3600) / 60)
  const s = secs % 60

  return h
      ? `${h}h ${m}m`
      : m
          ? `${m}m ${s}s`
          : `${s}s`
}

function formatFilament(raw?: number): string {
  if (raw == null || !Number.isFinite(raw)) return '—'

  return `${(raw / 1000).toLocaleString(undefined, {
    maximumFractionDigits: 2,
  })} m`
}

function statusName(key: string): string {
  const known: Record<string, string> = {
    completed: t('settings.print_history.completed'),
    cancelled: t('settings.print_history.cancelled'),
    error: t('settings.print_history.error'),
    klippy_shutdown: t('settings.print_history.shutdown'),
    interrupted: t('settings.print_history.interrupted'),
    in_progress: t('settings.print_history.in_progress'),
  }

  return known[key] ?? key.replace(/_/g, ' ')
}

let requestSerial = 0

async function refresh() {
  if (!connected.value) return

  const serial = ++requestSerial

  loading.value = true
  failed.value = false

  try {
    const [totalsResult, firstPage] = await Promise.all([
      moonraker.call<HistoryTotals>('server.history.totals'),
      moonraker.call<HistoryList>('server.history.list', {
        limit: 500,
        start: 0,
        order: 'desc',
      }),
    ])

    const jobs = [...(firstPage.jobs ?? [])]
    const expected = Math.min(firstPage.count ?? jobs.length, 10000)

    while (jobs.length < expected) {
      const page = await moonraker.call<HistoryList>(
          'server.history.list',
          {
            limit: 500,
            start: jobs.length,
            order: 'desc',
          }
      )

      if (!page.jobs?.length) break
      jobs.push(...page.jobs)
    }

    if (serial !== requestSerial) return

    const next: Record<string, number> = {}

    for (const job of jobs) {
      const key = job.status || 'other'
      next[key] = (next[key] ?? 0) + 1
    }

    counts.value = next
    totals.value = totalsResult.job_totals ?? {}
  } catch (err) {
    if (serial === requestSerial) {
      failed.value = true
      console.warn(
          'Unable to load Moonraker print statistics:',
          err
      )
    }
  } finally {
    if (serial === requestSerial) loading.value = false
  }
}

let interval: ReturnType<typeof setInterval> | undefined

watch(
    connected,
    (online) => {
      if (online) void refresh()
    },
    { immediate: true }
)

onMounted(() => {
  interval = setInterval(() => {
    if (connected.value) void refresh()
  }, 60000)
})

onUnmounted(() => {
  requestSerial++
  if (interval) clearInterval(interval)
})
</script>

<template>
  <v-card rounded="lg" variant="tonal" class="history-card">
    <v-card-text class="pa-3">
      <div class="history-header">
        <div class="history-title">
          {{ t('settings.print_history.title') }}
        </div>

        <v-btn-toggle
            v-model="view"
            mandatory
            density="compact"
            variant="tonal"
            color="primary"
            class="history-view-toggle"
        >
          <v-btn
              value="chart"
              icon="mdi-chart-donut"
              size="small"
          />

          <v-btn
              value="details"
              icon="mdi-format-list-bulleted"
              size="small"
          />
        </v-btn-toggle>
      </div>

      <div
          v-if="failed"
          class="text-caption text-medium-emphasis mb-2"
      >
        {{ t('settings.print_history.unavailable') }}
      </div>

      <div v-if="view === 'details'" class="history-metrics">
        <div class="history-metric">
          <span>
            {{ t('settings.print_history.total_print_time') }}
          </span>
          <strong>
            {{ formatDuration(totals.total_print_time) }}
          </strong>
        </div>

        <div class="history-metric">
          <span>
            {{ t('settings.print_history.longest_print') }}
          </span>
          <strong>
            {{ formatDuration(totals.longest_job) }}
          </strong>
        </div>

        <div class="history-metric">
          <span>
            {{ t('settings.print_history.average_print') }}
          </span>
          <strong>
            {{ formatDuration(averagePrintTime) }}
          </strong>
        </div>

        <div class="history-metric">
          <span>
            {{ t('settings.print_history.filament') }}
          </span>
          <strong>
            {{ formatFilament(totals.total_filament_used) }}
          </strong>
        </div>
      </div>

      <div v-else class="history-visual">
        <div class="donut-wrapper">
          <svg
              viewBox="0 0 100 100"
              class="history-donut"
              role="img"
          >
            <circle
                cx="50"
                cy="50"
                r="39"
                fill="none"
                stroke="currentColor"
                stroke-opacity=".12"
                stroke-width="14"
            />

            <circle
                v-for="segment in chartSegments"
                :key="segment.key"
                cx="50"
                cy="50"
                r="39"
                fill="none"
                :stroke="segment.color"
                stroke-width="14"
                pathLength="100"
                :stroke-dasharray="segment.dash"
                :stroke-dashoffset="segment.offset"
            />
          </svg>

          <div class="donut-center">
            <strong>{{ countedJobs }}</strong>
            <span>
              {{ t('settings.print_history.jobs') }}
            </span>
          </div>
        </div>

        <div
            v-if="countEntries.length"
            class="history-legend"
        >
          <div
              v-for="[key, n] in countEntries"
              :key="key"
              class="history-legend-row"
          >
            <span
                class="history-status"
                :style="{
                backgroundColor: colors[key] ?? '#90a4ae'
              }"
            />

            <span class="history-status-name">
              {{ statusName(key) }}
            </span>

            <strong>{{ n }}</strong>
          </div>
        </div>

        <div
            v-else
            class="text-caption text-medium-emphasis"
        >
          {{
            loading
                ? t('settings.print_history.loading')
                : t('settings.print_history.empty')
          }}
        </div>
      </div>
    </v-card-text>
  </v-card>
</template>

<style scoped>
.history-card {
  width: 100%;
  display: flex;
  flex-direction: column;
}

.history-card > :deep(.v-card-text) {
  flex: 1;
  display: flex;
  flex-direction: column;
}

.history-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 10px;
  margin-bottom: 10px;
  min-height: 30px;
}

.history-title {
  font-size: .9rem;
  font-weight: 500;
  line-height: 1.4;
  padding-top: 0;
}

.history-view-toggle {
  flex-shrink: 0;
  height: 30px;
}

.history-view-toggle :deep(.v-btn) {
  width: 36px;
  min-width: 36px;
  height: 30px;
}

.history-metrics {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 1px 0 3px;
}

.history-metric {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  font-size: .8rem;
}

.history-metric span {
  color: rgba(var(--v-theme-on-surface), .7);
}

.history-metric strong {
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.history-visual {
  min-width: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 16px;
  padding: 3px 0;
  flex: 1 1 auto;
}

.donut-wrapper {
  width: 112px;
  flex: 0 0 112px;
  aspect-ratio: 1;
  position: relative;
}

.history-donut {
  width: 100%;
  height: 100%;
  transform: rotate(-90deg);
}

.donut-center {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  line-height: 1.25;
}

.donut-center strong {
  font-size: 1rem;
}

.donut-center span {
  font-size: .64rem;
  opacity: .65;
}

.history-legend {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 5px;
}

.history-legend-row {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: .73rem;
}

.history-status {
  width: 8px;
  height: 8px;
  flex-shrink: 0;
  border-radius: 50%;
}

.history-status-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.history-legend-row strong {
  font-variant-numeric: tabular-nums;
}

@media (max-width: 420px) {
  .donut-wrapper {
    width: 92px;
    flex-basis: 92px;
  }

  .history-visual {
    gap: 10px;
  }
}
</style>
