<script setup lang="ts">
import { computed } from 'vue'
import { storeToRefs } from 'pinia'
import { useI18n } from 'vue-i18n'
import { useAppStore } from '@/stores/app'

const { t, locale } = useI18n()
const { moonraker } = storeToRefs(useAppStore())
type Values = Record<string, unknown>
const obj = (v: unknown): Values => v && typeof v === 'object' && !Array.isArray(v) ? v as Values : {}
const num = (v: unknown): number | null => typeof v === 'number' && Number.isFinite(v) ? v : null
const raw = computed(() => obj(moonraker.value.rawObjects))
const motion = computed(() => obj(raw.value.motion_report ?? (moonraker.value as any).motionReport))
const toolhead = computed(() => obj(raw.value.toolhead))
const gcode = computed(() => obj(raw.value.gcode_move))
const format = (n: number, digits = 1) => new Intl.NumberFormat(locale.value || 'en', { minimumFractionDigits: digits, maximumFractionDigits: digits }).format(n)
const actualSpeed = computed(() => num(motion.value.live_velocity))
const actualFlow = computed(() => {
  const v = num(motion.value.live_extruder_velocity)
  const config = obj(obj(raw.value.configfile).config)
  const d = num(obj(config.extruder).filament_diameter) ?? 1.75
  return v === null ? null : Math.max(0, v) * Math.PI * (d / 2) ** 2
})
const position = computed(() => {
  const values = Array.isArray(gcode.value.gcode_position) ? gcode.value.gcode_position :
    Array.isArray(moonraker.value.toolhead?.position) ? moonraker.value.toolhead.position : null
  return [0, 1, 2].map(i => values ? num(values[i]) : null)
})
const bounds = computed(() => [0, 1, 2].map(i => {
  const min = Array.isArray(toolhead.value.axis_minimum) ? num(toolhead.value.axis_minimum[i]) : null
  const max = Array.isArray(toolhead.value.axis_maximum) ? num(toolhead.value.axis_maximum[i]) : null
  return { min, max }
}))
const fraction = (i: number): number => {
  const value = position.value[i]
  const { min, max } = bounds.value[i]!
  if (value === null || min === null || max === null || max <= min) return .5
  return Math.max(0, Math.min(1, (value - min) / (max - min)))
}
const hasXY = computed(() => position.value[0] !== null && position.value[1] !== null && bounds.value.slice(0, 2).every(b => b.min !== null && b.max !== null && b.max > b.min))
const hasZ = computed(() => position.value[2] !== null && bounds.value[2]?.min !== null && bounds.value[2]?.max !== null && bounds.value[2]!.max! > bounds.value[2]!.min!)
const bedWidth = computed(() => Math.max(1, (bounds.value[0]?.max ?? 350) - (bounds.value[0]?.min ?? 0)))
const bedDepth = computed(() => Math.max(1, (bounds.value[1]?.max ?? 350) - (bounds.value[1]?.min ?? 0)))
// Constrain bed drawing to a 140 × 126 viewport without distorting actual bed proportions.
const bedScale = computed(() => Math.min(140 / bedWidth.value, 120 / bedDepth.value))
const bedW = computed(() => bedWidth.value * bedScale.value)
const bedH = computed(() => bedDepth.value * bedScale.value)
const bedX = computed(() => 12 + (140 - bedW.value) / 2)
const bedY = computed(() => 14 + (120 - bedH.value) / 2)
const headX = computed(() => bedX.value + bedW.value * fraction(0))
const headY = computed(() => bedY.value + bedH.value * (1 - fraction(1)))
const zY = computed(() => 134 - fraction(2) * 126)
const coordinate = (i: number) => position.value[i] === null ? '—' : format(position.value[i]!, 2)
const maxSpeed = computed(() => num(toolhead.value.max_velocity))
const maxAcceleration = computed(() => num(toolhead.value.max_accel))
const filamentLength = computed(() => num(moonraker.value.printStats?.filamentUsed))
const speed = computed(() => actualSpeed.value === null ? '—' : `${format(Math.max(0, actualSpeed.value))} mm/s`)
const flow = computed(() => actualFlow.value === null ? '—' : `${format(actualFlow.value, 2)} mm³/s`)
const filament = computed(() => filamentLength.value === null ? '—' : filamentLength.value >= 1000 ? `${format(filamentLength.value / 1000, 2)} m` : `${format(filamentLength.value, 2)} mm`)
// Moonraker proc_stats values are live host telemetry, not print head metrics.
const cpuTemp = computed(() => num(moonraker.value.procStats?.cpuTemp))
const cpuUsage = computed(() => {
  const value = moonraker.value.procStats?.systemCpuUsage
  return num(value) ?? num(obj(value).cpu)
})
const uptime = computed(() => num(moonraker.value.procStats?.systemUptime))
const hostTemperature = computed(() => cpuTemp.value === null ? '—' : `${format(cpuTemp.value, 0)} °C`)
const cpuTempClass = computed(() => {
  if (cpuTemp.value === null) return ''
  if (cpuTemp.value >= 85) return 'temp-danger'
  if (cpuTemp.value >= 70) return 'temp-warn'
  return ''
})
const hostLoad = computed(() => cpuUsage.value === null ? '—' : `${format(cpuUsage.value, 0)} %`)
const systemMemory = computed(() => obj(moonraker.value.procStats?.systemMemory))
const ramPercent = computed(() => {
  const total = num(systemMemory.value.total)
  const used = num(systemMemory.value.used)
  return total !== null && total > 0 && used !== null ? Math.min(100, Math.max(0, 100 * used / total)) : null
})
const ramUsage = computed(() => ramPercent.value === null ? '—' : `${format(ramPercent.value, 0)} %`)
const websocketCount = computed(() => {
  const value = num((moonraker.value.procStats as Values | undefined)?.websocket_connections)
  return value === null ? '—' : String(Math.round(value))
})
// Storage statistics are already populated by Moonraker's built-in
// server.files.get_directory(path: 'gcodes') request.
const storage = computed(() => obj((moonraker.value as any).storage))
const storagePercent = computed(() => {
  const total = num(storage.value.total)
  const used = num(storage.value.used)
  return total !== null && total > 0 && used !== null
    ? Math.min(100, Math.max(0, 100 * used / total))
    : null
})
const storageUsage = computed(() => storagePercent.value === null ? '—' : `${format(storagePercent.value, 0)} %`)
const hostUptime = computed(() => {
  if (uptime.value === null) return '—'
  const seconds = Math.max(0, Math.floor(uptime.value))
  const days = Math.floor(seconds / 86400)
  const hours = Math.floor((seconds % 86400) / 3600)
  const minutes = Math.floor((seconds % 3600) / 60)
  return days ? `${days}d ${hours}h` : hours ? `${hours}h ${minutes}m` : `${minutes}m`
})
</script>

<template>
  <v-card class="advanced-details-panel my-2 mr-2" rounded="lg" variant="flat">
    <section class="position-panel">
      <div class="position-graphic">
        <svg class="position-map" viewBox="0 0 202 156" role="img" aria-label="Top-down printer bed; X increases right, Y increases toward the back, Z is vertical">
          <!-- Actual X/Y travel proportions from Klipper axis bounds; viewed from printer front. -->
          <rect :x="bedX" :y="bedY" :width="bedW" :height="bedH" rx="3" class="bed" />
          <path v-for="i in 3" :key="`vx${i}`" :d="`M${bedX + bedW * i / 4} ${bedY} v${bedH}`" class="grid-lines" />
          <path v-for="i in 3" :key="`hy${i}`" :d="`M${bedX} ${bedY + bedH * i / 4} h${bedW}`" class="grid-lines" />
          <path v-if="hasXY" :d="`M${headX} ${bedY} V${bedY + bedH} M${bedX} ${headY} H${bedX + bedW}`" class="crosshair" />
          <circle v-if="hasXY" :cx="headX" :cy="headY" r="6.5" class="head-ring" />
          <circle v-if="hasXY" :cx="headX" :cy="headY" r="2.5" class="head-dot" />
          <text :x="bedX + bedW / 2" :y="bedY + bedH + 13" class="axis-label" text-anchor="middle">X</text>
          <text :x="bedX - 7" :y="bedY + bedH / 2" class="axis-label" text-anchor="end" dominant-baseline="middle">Y</text>
          <path d="M178 134 V8" class="z-track" />
          <path v-if="hasZ" :d="`M178 134 V${zY}`" class="z-fill" />
          <path v-if="hasZ" :d="`M170 ${zY} H186`" class="z-marker" />
          <text x="178" y="151" text-anchor="middle" class="axis-label">Z</text>
        </svg>
      </div>
      <div class="position-bottom">
        <div class="coordinate-row">
          <div class="coordinate-value"><span class="x-axis">X</span><strong>{{ coordinate(0) }}</strong></div>
          <div class="coordinate-value"><span class="y-axis">Y</span><strong>{{ coordinate(1) }}</strong></div>
          <div class="coordinate-value"><span class="z-axis">Z</span><strong>{{ coordinate(2) }}</strong></div>
        </div>
        <div class="motion-values">
          <div><span>{{ t('advanced_details.speed') }}</span><strong>{{ speed }}</strong></div>
          <div><span>Max {{ t('advanced_details.velocity') }}</span><strong>{{ maxSpeed === null ? '—' : `${format(maxSpeed, 0)} mm/s` }}</strong></div>
          <div><span>Max {{ t('advanced_details.acceleration') }}</span><strong>{{ maxAcceleration === null ? '—' : `${format(maxAcceleration, 0)} mm/s²` }}</strong></div>
        </div>
      </div>
    </section>
    <section class="material-panel">
      <div class="material-value"><span>{{ t('advanced_details.flow') }}</span><strong>{{ flow }}</strong></div>
      <div class="material-divider"></div>
      <div class="material-value"><span>{{ t('advanced_details.filament_length') }}</span><strong>{{ filament }}</strong></div>
    </section>
    <section class="system-panel" aria-label="Printer host telemetry">
      <div class="system-grid">
        <div class="system-metric system-usage cpu-metric">
          <div class="cpu-header-row">
            <span>CPU</span>
            <strong class="cpu-temp" :class="cpuTempClass">{{ hostTemperature }}</strong>
          </div>
          <div class="cpu-load-row">
            <span>Load</span>
            <strong>{{ hostLoad }}</strong>
          </div>
          <div class="metric-track"><div class="metric-fill" :style="{ width: `${cpuUsage ?? 0}%` }" /></div>
        </div>
        <div class="system-metric">
          <span>Uptime</span>
          <strong>{{ hostUptime }}</strong>
        </div>
        <div class="system-metric system-usage">
          <span>RAM</span>
          <strong>{{ ramUsage }}</strong>
          <div class="metric-track"><div class="metric-fill" :style="{ width: `${ramPercent ?? 0}%` }" /></div>
        </div>
        <div class="system-metric">
          <span>WebSockets</span>
          <strong>{{ websocketCount }}</strong>
        </div>
        <div class="system-metric system-usage">
          <span>Storage</span>
          <strong>{{ storageUsage }}</strong>
          <div class="metric-track"><div class="metric-fill" :style="{ width: `${storagePercent ?? 0}%` }" /></div>
        </div>
      </div>
    </section>
  </v-card>
</template>

<style scoped>
.advanced-details-panel {
  width: 260px;
  height: calc(100% - 16px);
  max-height: calc(100dvh - 16px);
  min-height: 0;
  padding: 6px;
  display: flex;
  flex-direction: column;
  gap: 5px;
  overflow: hidden;
  background: rgb(var(--v-theme-surface));
  color: rgb(var(--v-theme-on-surface));
}
.position-panel, .material-panel {
  border-radius: 9px;
  background: rgba(var(--v-theme-on-surface), .065);
  min-height: 0;
}
.position-panel { padding: 8px; flex: 0 0 auto; display: flex; flex-direction: column; gap: 5px; overflow: hidden; }
.position-graphic { flex: 0 0 174px; height: 174px; min-height: 0; display: flex; align-items: center; justify-content: center; overflow: hidden; }
.position-map { display: block; height: 100%; max-height: 100%; width: 100%; }
.position-bottom { flex: 0 0 auto; }
.coordinate-row { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 4px; }
.coordinate-value { min-width: 0; border-radius: 5px; background: rgba(var(--v-theme-on-surface), .065); padding: 4px 5px; display: flex; flex-direction: column; gap: 1px; }
.coordinate-value span { font-size: .68rem; font-weight: 750; }
.coordinate-value strong { font-size: .83rem; font-weight: 750; font-variant-numeric: tabular-nums; white-space: nowrap; }
.motion-values { display: flex; flex-direction: column; align-items: stretch; gap: 3px; padding: 5px 2px 0; }
.motion-values > div { display: flex; justify-content: space-between; align-items: baseline; gap: 7px; width: 100%; line-height: 1.2; }
.motion-values span { font-size: .68rem; opacity: .7; }
.motion-values strong { min-width: 95px; text-align: right; font-size: .83rem; font-weight: 700; font-variant-numeric: tabular-nums; }
.material-panel { flex: 0 0 auto; display: grid; grid-template-columns: minmax(0, 1fr) 1px minmax(0, 1fr); gap: 7px; padding: 9px 8px; align-items: stretch; }
.material-value { display: flex; flex-direction: column; gap: 3px; min-width: 0; }
.material-value span { font-size: .63rem; opacity: .74; white-space: nowrap; }
.material-value strong { font-size: .82rem; font-weight: 720; white-space: nowrap; font-variant-numeric: tabular-nums; }
.material-divider { background: rgba(var(--v-theme-on-surface), .16); }
.bed { fill: rgba(var(--v-theme-primary), .035); stroke: rgba(var(--v-theme-on-surface), .57); stroke-width: 1.4; }
.grid-lines { fill: none; stroke: rgba(var(--v-theme-on-surface), .15); stroke-width: .7; }
.crosshair { fill: none; stroke: rgb(var(--v-theme-primary)); stroke-width: 1.2; stroke-dasharray: 3 3; opacity: .8; }
.head-ring { fill: rgb(var(--v-theme-surface)); stroke: rgb(var(--v-theme-primary)); stroke-width: 2.5; }
.head-dot { fill: rgb(var(--v-theme-primary)); }
.z-track { stroke: rgba(var(--v-theme-on-surface), .25); stroke-width: 6; stroke-linecap: round; }
.z-fill { stroke: rgb(var(--v-theme-success)); stroke-width: 6; stroke-linecap: round; }
.z-marker { stroke: rgb(var(--v-theme-on-surface)); stroke-width: 2; stroke-linecap: round; }
.axis-label { font-size: 10px; font-weight: 700; fill: rgb(var(--v-theme-on-surface)); }
.x-axis { color: rgb(var(--v-theme-primary)); }
.y-axis { color: rgb(var(--v-theme-secondary)); }
.z-axis { color: rgb(var(--v-theme-success)); }
</style>

<style scoped>
/* Unified host diagnostics with a compact 2-column layout. */
.advanced-details-panel { gap: 3px; padding: 4px; }
.position-panel { padding: 5px 7px; gap: 3px; }
/* Position is the elastic section; the other cards keep their content height. */
.position-panel {
  flex: 1 1 0;
  min-height: 0;
  justify-content: flex-start;
}
.position-graphic {
  flex: 1 1 0;
  height: auto;
  min-height: 0;
  width: 100%;
}
/* Preserve the bed's proportions while letting SVG grow with the card. */
.position-map {
  width: 100%;
  height: 100%;
  max-width: 100%;
  max-height: 100%;
}
.coordinate-value { padding: 2px 4px; }
.motion-values { gap: 1px; padding-top: 2px; }
.material-panel { padding: 5px 7px; }
.system-panel {
  flex: 0 0 auto;
  border-radius: 9px;
  background: rgba(var(--v-theme-on-surface), .065);
  padding: 3px 5px;
  min-width: 0;
}
.system-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 3px;
}
.system-metric {
  min-width: 0;
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: 1px;
  padding: 2px 5px;
  border-radius: 7px;
  background: rgba(var(--v-theme-surface), .32);
  border: 1px solid rgba(var(--v-theme-on-surface), .08);
}
.system-metric span {
  font-size: .62rem;
  opacity: .72;
  white-space: nowrap;
  line-height: 1.05;
}
.system-metric strong {
  font-size: .84rem;
  font-weight: 760;
  white-space: nowrap;
  font-variant-numeric: tabular-nums;
  line-height: 1.25;
}
.system-usage .metric-track {
  height: 3px;
  width: 100%;
  background: rgba(var(--v-theme-on-surface), .12);
  border-radius: 5px;
  overflow: hidden;
  margin-top: 2px;
}
.metric-fill {
  background: rgb(var(--v-theme-primary));
  height: 100%;
  border-radius: inherit;
  max-width: 100%;
  transition: width .4s ease;
}

.cpu-metric {
  gap: 2px;
}
.cpu-header-row,
.cpu-load-row {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 8px;
}
.cpu-header-row span,
.cpu-load-row span {
  font-size: .62rem;
  opacity: .72;
  white-space: nowrap;
  line-height: 1.05;
}
.cpu-header-row .cpu-temp,
.cpu-load-row strong {
  text-align: right;
}
.system-metric .cpu-temp {
  font-size: .62rem;
  transition: color .25s ease, text-shadow .25s ease, filter .25s ease;
}
.temp-warn {
  color: rgb(var(--v-theme-warning));
  text-shadow: 0 0 10px rgba(var(--v-theme-warning), .35);
}
.temp-danger {
  color: rgb(var(--v-theme-error));
  text-shadow: 0 0 12px rgba(var(--v-theme-error), .45);
}
</style>
