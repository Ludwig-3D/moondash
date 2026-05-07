<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { storeToRefs } from 'pinia'
import { useI18n } from 'vue-i18n'
import { moonraker as moonrakerClient } from '@/plugins/moonraker'
import { useAppStore } from '@/stores/app'

type PointLike =
    | [number, number]
    | { x?: number; y?: number }

type ExcludeObjectDef = {
  name?: string
  polygon?: PointLike[]
  center?: PointLike
  location?: PointLike
}

type PreviewObject = {
  name: string
  excluded: boolean
  current: boolean
  selected: boolean
  points: Array<{ x: number; y: number }>
  marker: { x: number; y: number } | null
  hasPreview: boolean
}

const { t } = useI18n()
const appStore = useAppStore()
const { moonraker } = storeToRefs(appStore)

const props = defineProps<{
  modelValue: boolean
}>()

const emit = defineEmits<{
  (e: 'update:modelValue', value: boolean): void
}>()

const loading = ref(false)
const selectedObjectName = ref('')

const dialogOpen = computed({
  get: () => props.modelValue,
  set: (value: boolean) => emit('update:modelValue', value),
})

const excludeState = computed(() => {
  const raw = moonraker.value.rawObjects['exclude_object']
  return raw && typeof raw === 'object' && !Array.isArray(raw)
      ? (raw as Record<string, unknown>)
      : {}
})

const excludedNames = computed(() => {
  const excluded = excludeState.value.excluded_objects
  return Array.isArray(excluded) ? excluded.map(String) : []
})

const currentObject = computed(() => {
  const value = excludeState.value.current_object
  return typeof value === 'string' ? value : ''
})

function normalizePoint(point: PointLike | null | undefined) {
  if (!point) return null

  let normalized: { x: number; y: number } | null = null

  if (Array.isArray(point)) {
    normalized = { x: Number(point[0]), y: Number(point[1]) }
  } else if (typeof point === 'object') {
    normalized = { x: Number(point.x), y: Number(point.y) }
  }

  return normalized && Number.isFinite(normalized.x) && Number.isFinite(normalized.y)
      ? normalized
      : null
}

function polygonCenter(points: Array<{ x: number; y: number }>) {
  if (!points.length) return null

  return {
    x: points.reduce((sum, point) => sum + point.x, 0) / points.length,
    y: points.reduce((sum, point) => sum + point.y, 0) / points.length,
  }
}

const objectItems = computed(() => {
  const objects = excludeState.value.objects
  if (!Array.isArray(objects)) return []

  const items = (objects as ExcludeObjectDef[])
      .map((item) => {
        const name = String(item?.name ?? '').trim()
        if (!name) return null

        const polygon = Array.isArray(item?.polygon)
            ? item.polygon.map(normalizePoint).filter((p): p is { x: number; y: number } => Boolean(p))
            : []

        const isCurrent = currentObject.value === name
        const marker = normalizePoint(item?.center)
            ?? normalizePoint(item?.location)
            ?? polygonCenter(polygon)
            ?? (isCurrent ? { x: 0, y: 0 } : null)

        return {
          name,
          excluded: excludedNames.value.includes(name),
          current: isCurrent,
          polygon,
          marker,
        }
      })
      .filter((item): item is NonNullable<typeof item> => Boolean(item))

  if (currentObject.value && !items.some((item) => item.name === currentObject.value)) {
    items.unshift({
      name: currentObject.value,
      excluded: excludedNames.value.includes(currentObject.value),
      current: true,
      polygon: [],
      marker: { x: 0, y: 0 },
    })
  }

  return items
})

watch(
    () => [props.modelValue, objectItems.value.map((item) => item.name).join('|'), currentObject.value],
    ([open]) => {
      if (!open) return

      const names = objectItems.value.map((item) => item.name)
      if (selectedObjectName.value && names.includes(selectedObjectName.value)) return

      const preferred =
          currentObject.value && names.includes(currentObject.value)
              ? currentObject.value
              : names[0] ?? ''

      selectedObjectName.value = preferred
    },
    { immediate: true },
)

const selectedObject = computed(() => {
  return objectItems.value.find((item) => item.name === selectedObjectName.value) ?? null
})

const previewObjects = computed<PreviewObject[]>(() => {
  return objectItems.value
      .map((item) => ({
        name: item.name,
        excluded: item.excluded,
        current: item.current,
        selected: selectedObjectName.value === item.name,
        points: item.polygon,
        marker: item.marker,
        hasPreview: item.polygon.length >= 3,
      }))
      .sort((a, b) => Number(b.excluded) - Number(a.excluded) || Number(a.current) - Number(b.current))
})

function numericConfigValue(value: unknown) {
  const parsed = Number(value)
  return Number.isFinite(parsed) ? parsed : null
}

function configSection(name: string) {
  const configfile = moonraker.value.rawObjects['configfile'] as Record<string, unknown> | undefined
  const config = configfile && typeof configfile === 'object'
      ? configfile.config as Record<string, unknown> | undefined
      : undefined

  const section = config?.[name]
  return section && typeof section === 'object' && !Array.isArray(section)
      ? section as Record<string, unknown>
      : {}
}

const printerBounds = computed(() => {
  const stepperX = configSection('stepper_x')
  const stepperY = configSection('stepper_y')

  const configuredMinX = numericConfigValue(stepperX.position_min)
  const configuredMaxX = numericConfigValue(stepperX.position_max)
  const configuredMinY = numericConfigValue(stepperY.position_min)
  const configuredMaxY = numericConfigValue(stepperY.position_max)

  const allPoints = previewObjects.value.flatMap((item) => [
    ...item.points,
    ...(item.marker ? [item.marker] : []),
    { x: 0, y: 0 },
  ])

  const minObjectX = allPoints.length ? Math.min(...allPoints.map((p) => p.x)) : 0
  const maxObjectX = allPoints.length ? Math.max(...allPoints.map((p) => p.x)) : 100
  const minObjectY = allPoints.length ? Math.min(...allPoints.map((p) => p.y)) : 0
  const maxObjectY = allPoints.length ? Math.max(...allPoints.map((p) => p.y)) : 100

  const minX = configuredMinX ?? Math.min(0, minObjectX)
  const maxX = configuredMaxX ?? Math.max(100, maxObjectX)
  const minY = configuredMinY ?? Math.min(0, minObjectY)
  const maxY = configuredMaxY ?? Math.max(100, maxObjectY)

  return {
    minX,
    maxX,
    minY,
    maxY,
    width: Math.max(1, maxX - minX),
    height: Math.max(1, maxY - minY),
  }
})

const previewBounds = computed(() => {
  const bounds = printerBounds.value
  const padding = Math.max(bounds.width, bounds.height) * 0.04

  return {
    minX: bounds.minX - padding,
    minY: bounds.minY - padding,
    width: bounds.width + padding * 2,
    height: bounds.height + padding * 2,
  }
})
const hasAnyPreview = computed(() => previewObjects.value.some((item) => item.hasPreview || item.marker))

function svgX(x: number): number {
  return x
}

function svgY(y: number): number {
  const bounds = printerBounds.value
  return bounds.minY + bounds.maxY - y
}

function pointsToSvg(points: Array<{ x: number; y: number }>): string {
  return points.map((p) => `${svgX(p.x)},${svgY(p.y)}`).join(' ')
}

const markerRadius = computed(() => Math.max(printerBounds.value.width, printerBounds.value.height) * 0.018)

function svgPoint(point: { x: number; y: number }) {
  return {
    x: svgX(point.x),
    y: svgY(point.y),
  }
}

function selectObject(name: string) {
  selectedObjectName.value = name
}

async function skipSelectedObject() {
  const name = selectedObject.value?.name
  if (!name || loading.value || selectedObject.value?.excluded) return

  try {
    loading.value = true
    await moonrakerClient.call('printer.gcode.script', {
      script: `EXCLUDE_OBJECT NAME=${JSON.stringify(name)}`,
    })
    dialogOpen.value = false
  } finally {
    loading.value = false
  }
}
</script>

<template>
  <v-dialog v-model="dialogOpen" max-width="1200" persistent>
    <v-card rounded="lg" class="pa-0">
      <v-card-text class="pa-0">
        <div v-if="!objectItems.length" class="skip-object-empty">
          {{ t('print.current.no_objects') }}
        </div>

        <div v-else class="skip-object-layout">
          <div class="skip-object-left">
            <div class="skip-object-preview">
              <svg
                  v-if="hasAnyPreview"
                  class="skip-object-preview__svg"
                  :viewBox="`${previewBounds.minX} ${previewBounds.minY} ${previewBounds.width} ${previewBounds.height}`"
                  preserveAspectRatio="xMidYMid meet"
              >
                <g v-for="item in previewObjects" :key="item.name">
                  <polygon
                      v-if="item.hasPreview"
                      :points="pointsToSvg(item.points)"
                      class="skip-object-preview__polygon"
                      :class="{
                      'skip-object-preview__polygon--selected': item.selected && !item.current && !item.excluded,
                      'skip-object-preview__polygon--current': item.current && !item.excluded,
                      'skip-object-preview__polygon--excluded': item.excluded,
                    }"
                      @click="selectObject(item.name)"
                  />

                  <circle
                      v-if="!item.hasPreview && item.marker"
                      :cx="svgPoint(item.marker).x"
                      :cy="svgPoint(item.marker).y"
                      :r="markerRadius"
                      class="skip-object-preview__marker"
                      :class="{
                      'skip-object-preview__marker--selected': item.selected && !item.current && !item.excluded,
                      'skip-object-preview__marker--current': item.current && !item.excluded,
                      'skip-object-preview__marker--excluded': item.excluded,
                    }"
                      @click="selectObject(item.name)"
                  />
                </g>

              </svg>

              <div v-else class="skip-object-preview__empty">
                {{ t('print.current.no_preview') }}
              </div>
            </div>
          </div>

          <div class="skip-object-right">
            <div class="skip-object-list-wrap">
              <v-list class="skip-object-list" density="compact" bg-color="transparent">
                <v-list-item
                    v-for="item in previewObjects"
                    :key="item.name"
                    class="skip-object-item"
                    :class="{
                    'skip-object-item--selected': item.selected && !item.current,
                    'skip-object-item--current': item.current,
                    'skip-object-item--excluded': item.excluded,
                  }"
                    :active="false"
                    @click="selectObject(item.name)"
                >
                  <v-list-item-title class="skip-object-item__name">
                    {{ item.name }}
                  </v-list-item-title>

                  <v-list-item-subtitle class="skip-object-item__meta">
                    <span v-if="item.current">{{ t('print.current.current_object') }}</span>
                    <span v-else-if="item.excluded">{{ t('print.current.already_skipped') }}</span>
                    <span v-else>{{ t('print.current.select_object') }}</span>
                  </v-list-item-subtitle>
                </v-list-item>
              </v-list>
            </div>

            <div class="skip-object-actions">
              <v-btn variant="text" @click="dialogOpen = false">
                {{ t('print.current.close') }}
              </v-btn>
              <v-btn
                  color="secondary"
                  variant="text"
                  :disabled="loading || !selectedObject || selectedObject.excluded"
                  @click="skipSelectedObject"
              >
                {{ t('print.current.skip') }}
              </v-btn>
            </div>
          </div>
        </div>
      </v-card-text>
    </v-card>
  </v-dialog>
</template>

<style scoped>
.skip-object-empty {
  padding: 24px;
}

.skip-object-layout {
  display: grid;
  grid-template-columns: minmax(360px, 1.15fr) minmax(320px, 1fr);
  align-items: stretch;
}

.skip-object-left {
  position: relative;
  display: flex;
  flex-direction: column;
  min-height: 0;
  height: 100%;
}

.skip-object-preview {
  flex: 1;
  height: calc(100vh - 50px);
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 18px;
  background: rgba(var(--v-theme-on-surface), 0.04);
}

.skip-object-preview__svg {
  width: 100%;
  height: 100%;
  max-width: 420px;
  max-height: 420px;
}

.skip-object-preview__empty {
  opacity: 0.7;
}


.skip-object-preview__marker {
  fill: rgba(var(--v-theme-primary), 0.3);
  stroke: rgba(var(--v-theme-primary), 0.9);
  stroke-width: 1.8;
  cursor: pointer;
  transition: fill 0.16s ease, stroke 0.16s ease, opacity 0.16s ease;
  vector-effect: non-scaling-stroke;
}

.skip-object-preview__marker--selected {
  stroke: rgba(var(--v-theme-primary), 1);
  stroke-width: 2.8;
}

.skip-object-preview__marker--current {
  fill: rgba(var(--v-theme-secondary), 0.34);
  stroke: rgba(var(--v-theme-secondary), 1);
  stroke-width: 2.8;
}

.skip-object-preview__marker--excluded {
  fill: rgba(var(--v-theme-on-surface), 0.18);
  stroke: rgba(var(--v-theme-on-surface), 0.44);
  opacity: 0.5;
}

.skip-object-preview__polygon {
  fill: rgba(var(--v-theme-primary), 0.2);
  stroke: rgba(var(--v-theme-primary), 0.32);
  stroke-width: 1.2;
  cursor: pointer;
  transition: fill 0.16s ease, stroke 0.16s ease, opacity 0.16s ease;
}

.skip-object-preview__polygon--selected {
  stroke: rgba(var(--v-theme-primary), 1);
  stroke-width: 2.4;
}

.skip-object-preview__polygon--current {
  fill: rgba(var(--v-theme-secondary), 0.34);
  stroke: rgba(var(--v-theme-secondary), 1);
  stroke-width: 2.8;
}

.skip-object-preview__polygon--excluded {
  fill: rgba(var(--v-theme-on-surface), 0.14);
  stroke: rgba(var(--v-theme-on-surface), 0.28);
  opacity: 0.42;
}

.skip-object-right {
  display: flex;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
  height: 100%;
  background: rgba(var(--v-theme-on-surface), 0.06);
  padding: 12px;
}

.skip-object-list-wrap {
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
  overflow-x: hidden;
  max-height: calc(100vh - 140px);
  height: calc(100vh - 140px);
}

.skip-object-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 0;
  background: transparent !important;
}

.skip-object-item {
  border-radius: 5px;
  margin-bottom: 0;
  background: rgba(var(--v-theme-on-surface), 0.05);
  transition: background 0.16s ease, opacity 0.16s ease, border-color 0.16s ease;
  border-left: 3px solid transparent;
  cursor: pointer;
}

.skip-object-item--selected {
  border-left-color: rgba(var(--v-theme-primary), 1);
  background: rgba(var(--v-theme-primary), 0.1);
}

.skip-object-item--current {
  border-left-color: rgba(var(--v-theme-secondary), 1);
}

.skip-object-item--excluded {
  background: rgba(var(--v-theme-on-surface), 0.1);
  opacity: 0.55;
}

.skip-object-item__name {
  font-weight: 700;
  word-break: break-word;
}

.skip-object-item__meta {
  font-size: 0.85rem;
  opacity: 0.78;
}

.skip-object-actions {
  margin-top: auto;
  display: flex;
  justify-content: flex-end;
  gap: 12px;
  padding-top: 18px;
}

.skip-object-list :deep(.v-list-item) {
  border-bottom: none !important;
}

.skip-object-list :deep(.v-list-item__append),
.skip-object-list :deep(.v-list-item__prepend) {
  align-self: center;
}

.skip-object-list :deep(.v-list-item__overlay) {
  display: none;
}
</style>