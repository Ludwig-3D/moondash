<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

type AfcLane = {
  key: string
  id: string
  label: string
  color: string
  hasFilament: boolean
  isLoaded: boolean
  map: string
  material: string
  weight: number | null
}

type AfcUnit = {
  id: string
  label: string
  displayLabel: string
  lanes: AfcLane[]
}

const props = defineProps<{
  modelValue: boolean
  tool: string | null
  afcObjects: Record<string, unknown>
  selectedLaneKey?: string | null
  saving?: boolean
}>()

const emit = defineEmits<{
  (e: 'update:modelValue', value: boolean): void
  (e: 'select', laneKey: string): void
}>()

const { t } = useI18n()
const selectedUnitId = ref<string | null>(null)

const dialogOpen = computed({
  get: () => props.modelValue,
  set: (value: boolean) => emit('update:modelValue', value),
})

function normalizeColor(color: unknown): string {
  if (typeof color !== 'string' || !color.trim()) {
    return '#434343'
  }

  return color.trim()
}

function hasLaneFilament(lane: Record<string, unknown>): boolean {
  return Boolean(
      lane.load ||
      lane.prep ||
      lane.loaded_to_hub ||
      lane.tool_loaded
  )
}

function isLaneCurrentlyLoaded(
    laneName: string,
    lane: Record<string, unknown>,
    afcRoot: Record<string, any>,
    objects: Record<string, any>,
): boolean {
  if (lane.tool_loaded === true) return true
  if (lane.status === 'Tooled') return true
  if (afcRoot.current_load === laneName) return true

  const extruderObjects = Object.entries(objects).filter(([key]) =>
      key.startsWith('AFC_extruder ')
  )

  for (const [, extruderObject] of extruderObjects) {
    if (extruderObject?.lane_loaded === laneName) {
      return true
    }
  }

  return false
}

function parseColorToRgb(color: string): { r: number; g: number; b: number } | null {
  const value = color.trim().toLowerCase()

  if (!value.startsWith('#')) return null

  const hex = value.slice(1)

  if (hex.length === 3) {
    return {
      r: parseInt(hex[0] + hex[0], 16),
      g: parseInt(hex[1] + hex[1], 16),
      b: parseInt(hex[2] + hex[2], 16),
    }
  }

  if (hex.length === 6) {
    return {
      r: parseInt(hex.slice(0, 2), 16),
      g: parseInt(hex.slice(2, 4), 16),
      b: parseInt(hex.slice(4, 6), 16),
    }
  }

  return null
}

function isDarkColor(color: string): boolean {
  const rgb = parseColorToRgb(color)
  if (!rgb) return true

  const { r, g, b } = rgb
  const luminance = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255
  return luminance < 0.6
}

function getContrastTextColor(color: string): string {
  return isDarkColor(color) ? '#FFFFFF' : '#000000'
}

function getActionOverlayColor(color: string): string {
  return isDarkColor(color)
      ? 'rgba(255, 255, 255, 0.16)'
      : 'rgba(0, 0, 0, 0.12)'
}

function formatWeight(weight: number | null): string {
  if (typeof weight !== 'number' || !Number.isFinite(weight)) {
    return ''
  }

  return `${Math.round(weight)} g`
}

function getLaneLabel(laneName: string, laneObject: Record<string, unknown>): string {
  if (typeof laneObject.label === 'string' && laneObject.label.trim()) {
    return laneObject.label.trim()
  }

  if (typeof laneObject.name === 'string' && laneObject.name.trim()) {
    return laneObject.name.trim()
  }

  return laneName.replace(/^AFC_stepper\s+/i, '')
}

function getUnitDisplayLabel(unitName: string, unitObject: Record<string, unknown> | null): string {
  if (unitObject && typeof unitObject.label === 'string' && unitObject.label.trim()) {
    return unitObject.label.trim()
  }

  if (unitObject && typeof unitObject.name === 'string' && unitObject.name.trim()) {
    return unitObject.name.trim()
  }

  return unitName
}

function buildLane(laneName: string, afcRoot: Record<string, any>, objects: Record<string, any>): AfcLane {
  const laneKey = laneName.toLowerCase().startsWith('afc_stepper ')
      ? laneName
      : `AFC_stepper ${laneName}`

  const laneId = laneKey.replace(/^AFC_stepper\s+/i, '')
  const laneObject = objects[laneKey] ?? {}
  const hasFilament = hasLaneFilament(laneObject)
  const isLoaded = isLaneCurrentlyLoaded(laneId, laneObject, afcRoot, objects)
  const color = hasFilament ? normalizeColor(laneObject.color) : '#434343'

  return {
    key: laneKey,
    id: laneId,
    label: getLaneLabel(laneId, laneObject),
    color,
    hasFilament,
    isLoaded,
    map: typeof laneObject.map === 'string' ? laneObject.map : '',
    material:
        typeof laneObject.material === 'string' && laneObject.material.trim()
            ? laneObject.material
            : t('afc.line.empty'),
    weight:
        typeof laneObject.weight === 'number' && Number.isFinite(laneObject.weight)
            ? laneObject.weight
            : null,
  }
}

const parsedUnits = computed<AfcUnit[]>(() => {
  const objects = props.afcObjects as Record<string, any>
  const afcRoot = objects.AFC

  if (!afcRoot) return []

  if (Array.isArray(afcRoot.units) && afcRoot.units.length) {
    return afcRoot.units.map((unitName: string) => {
      const unitKey = `AFC_BoxTurtle ${unitName}`
      const unitObject = objects[unitKey] ?? null

      const laneNames: string[] = Array.isArray(unitObject?.lanes)
          ? unitObject.lanes
          : Array.isArray(afcRoot.lanes)
              ? afcRoot.lanes
              : []

      return {
        id: unitName,
        label: unitName,
        displayLabel: getUnitDisplayLabel(unitName, unitObject),
        lanes: laneNames.map((laneName) => buildLane(laneName, afcRoot, objects)),
      }
    })
  }

  const fallbackLaneNames: string[] = Array.isArray(afcRoot.lanes)
      ? afcRoot.lanes
      : Object.keys(objects).filter((key) => key.toLowerCase().startsWith('afc_stepper '))

  return [{
    id: 'AFC',
    label: 'AFC',
    displayLabel: 'AFC',
    lanes: fallbackLaneNames.map((laneName) => buildLane(laneName, afcRoot, objects)),
  }]
})

const currentUnit = computed(() => {
  return parsedUnits.value.find((unit) => unit.id === selectedUnitId.value)
      ?? parsedUnits.value[0]
      ?? null
})

const selectedLaneUnitId = computed(() => {
  if (!props.selectedLaneKey) return null

  for (const unit of parsedUnits.value) {
    if (unit.lanes.some((lane) => lane.key === props.selectedLaneKey)) {
      return unit.id
    }
  }

  return null
})

watch(
    () => [props.modelValue, parsedUnits.value.length, selectedLaneUnitId.value],
    () => {
      if (!props.modelValue) return

      selectedUnitId.value = selectedLaneUnitId.value
          ?? selectedUnitId.value
          ?? parsedUnits.value[0]?.id
          ?? null
    },
    { immediate: true },
)

function closeDialog() {
  dialogOpen.value = false
}

function selectLane(lane: AfcLane) {
  if (props.saving) return
  emit('select', lane.key)
}
</script>

<template>
  <v-dialog v-model="dialogOpen" max-width="960">
    <v-card rounded="lg" class="switch-lane-dialog">
      <v-card-title class="switch-lane-title">
        {{ t('print.dialog.switch_lane') }}
      </v-card-title>

      <v-card-text class="switch-lane-body">
        <v-row class="switch-lane-layout" no-gutters>
          <v-col cols="9" md="10" class="switch-lane-layout__lanes">
            <div v-if="currentUnit" class="afc-tray">
              <div class="afc-bars">
                <button
                    v-for="lane in currentUnit.lanes"
                    :key="lane.key"
                    type="button"
                    class="afc-bar"
                    :class="{
                    'afc-bar--loaded': lane.isLoaded,
                    'afc-bar--selected': selectedLaneKey === lane.key,
                    'afc-bar--empty': !lane.hasFilament,
                  }"
                    :style="{
                    backgroundColor: lane.color,
                    color: getContrastTextColor(lane.color),
                  }"
                    :disabled="saving"
                    @click="selectLane(lane)"
                >
                  <div
                      class="lane-top-info"
                      :style="{ background: getActionOverlayColor(lane.color) }"
                  >
                    <span>{{ lane.label }}</span>
                  </div>

                  <div
                      class="lane-center"
                      :style="{ color: getContrastTextColor(lane.color) }"
                  >
                    <div class="lane-material">
                      {{ lane.material }}
                    </div>

                    <div
                        v-if="lane.hasFilament && lane.weight !== null"
                        class="lane-weight"
                    >
                      {{ formatWeight(lane.weight) }}
                    </div>

                    <div
                        v-if="lane.map"
                        class="lane-map"
                    >
                      {{ lane.map }}
                    </div>
                  </div>
                </button>
              </div>
            </div>

            <v-alert
                v-else
                type="info"
                variant="tonal"
            >
              {{ t('afc.no_unit_selected') }}
            </v-alert>
          </v-col>

          <v-col cols="3" md="2" class="switch-lane-layout__units">
            <v-card class="unit-card pa-0">
              <v-list
                  v-if="parsedUnits.length"
                  class="unit-list"
                  density="compact"
                  nav
                  variant="tonal"
                  color="primary"
              >
                <v-list-item
                    v-for="unit in parsedUnits"
                    :key="unit.id"
                    :active="selectedUnitId === unit.id"
                    rounded="lg"
                    @click="selectedUnitId = unit.id"
                >
                  <v-list-item-title>
                    {{ unit.displayLabel }}
                  </v-list-item-title>
                </v-list-item>
              </v-list>

              <v-card-text v-else>
                <v-alert type="info" variant="tonal">
                  {{ t('afc.no_units') }}
                </v-alert>
              </v-card-text>
            </v-card>
          </v-col>
        </v-row>
      </v-card-text>

      <v-card-actions class="switch-lane-actions">
        <v-spacer />
        <v-btn
            variant="text"
            :disabled="saving"
            @click="closeDialog"
        >
          {{ t('print.dialog.close') }}
        </v-btn>
      </v-card-actions>
    </v-card>
  </v-dialog>
</template>

<style scoped>
.switch-lane-dialog {
  overflow: hidden;
}

.switch-lane-title {
  font-weight: 700;
}

.switch-lane-body {
  padding-top: 8px;
  padding-bottom: 8px;
}

.switch-lane-layout {
  min-height: calc(100vh - 175px);
}

.switch-lane-layout__lanes {
  padding-right: 12px;
  min-width: 0;
}

.switch-lane-layout__units {
  min-width: 0;
}

.unit-card {
  height: 100%;
  min-height: 0;
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

.unit-list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 0;
  background-color: rgba(var(--v-theme-on-surface), 0.12);
}

.afc-tray {
  height: 100%;
  width: 100%;
  padding: 0;
  margin: 0;
  box-sizing: border-box;
}

.afc-bars {
  display: flex;
  align-items: stretch;
  justify-content: space-around;
  height: 100%;
  gap: 10px;
}

.afc-bar {
  appearance: none;
  box-shadow: 0px 0px 3px 0px rgb(0 0 0 / 0.33);
  position: relative;
  height: 100%;
  width: 6.25rem;
  border-radius: 12px;
  border: 2px solid transparent;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  padding: 0;
  overflow: hidden;
  transition: transform 120ms ease, border-color 120ms ease, opacity 120ms ease;
}

.afc-bar:hover:not(:disabled) {
  transform: translateY(-2px);
}

.afc-bar:disabled {
  cursor: default;
  opacity: 0.72;
}

.afc-bar--loaded {
  border-color: rgb(var(--v-theme-primary));
}

.afc-bar--selected {
  border-color: rgb(var(--v-theme-primary));
  box-shadow: 0 0 0 2px rgba(var(--v-theme-primary), 0.28);
}

.afc-bar--empty {
  opacity: 0.82;
}

.lane-top-info {
  border-top-left-radius: 10px;
  border-top-right-radius: 10px;
  position: absolute;
  top: 0;
  width: 100%;
  min-height: 30px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 0.8rem;
  font-weight: 700;
  padding: 0 6px;
  box-sizing: border-box;
}

.lane-center {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  min-width: 0;
  max-width: calc(100% - 12px);
  text-align: center;
  padding-top: 22px;
}

.lane-selected-icon {
  margin-bottom: 8px;
}

.lane-material {
  font-size: 18px;
  font-weight: 600;
  min-width: 0;
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.lane-weight,
.lane-map {
  margin-top: 4px;
  font-size: 12px;
  font-weight: 500;
  opacity: 0.9;
  min-width: 0;
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.switch-lane-actions {
  padding: 8px 16px 14px;
}
</style>
