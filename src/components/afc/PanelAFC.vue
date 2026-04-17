<script setup lang="ts">
import { computed, ref } from 'vue'
import { storeToRefs } from 'pinia'
import { useAppStore } from '@/stores/app'
import { useI18n } from 'vue-i18n'
import { moonraker as moonrakerClient } from '@/plugins/moonraker'
import LaneDialogAFC from '@/components/afc/LaneDialogAFC.vue'

type AfcLane = {
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
  lanes: AfcLane[]
}

const props = defineProps<{
  selectedUnit?: string | null
}>()

const { t } = useI18n()
const appStore = useAppStore()
const { moonraker } = storeToRefs(appStore)

const ejectingLaneIds = ref<string[]>([])
const loadingLaneIds = ref<string[]>([])
const laneDialogOpen = ref(false)
const selectedLaneForDialog = ref<AfcLane | null>(null)

function normalizeColor(color: unknown): string {
  if (typeof color !== 'string' || !color.trim()) {
    return '#434343'
  }
  return color
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

  if (value.startsWith('#')) {
    const hex = value.slice(1)

    if (hex.length === 3) {
      const r = parseInt(hex[0] + hex[0], 16)
      const g = parseInt(hex[1] + hex[1], 16)
      const b = parseInt(hex[2] + hex[2], 16)
      return { r, g, b }
    }

    if (hex.length === 6) {
      const r = parseInt(hex.slice(0, 2), 16)
      const g = parseInt(hex.slice(2, 4), 16)
      const b = parseInt(hex.slice(4, 6), 16)
      return { r, g, b }
    }

    return null
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

function isEjecting(laneId: string): boolean {
  return ejectingLaneIds.value.includes(laneId)
}

function setLaneEjecting(laneId: string, value: boolean) {
  if (value) {
    if (!ejectingLaneIds.value.includes(laneId)) {
      ejectingLaneIds.value = [...ejectingLaneIds.value, laneId]
    }
    return
  }

  ejectingLaneIds.value = ejectingLaneIds.value.filter((id) => id !== laneId)
}

function isLoading(laneId: string): boolean {
  return loadingLaneIds.value.includes(laneId)
}

function setLaneLoading(laneId: string, value: boolean) {
  if (value) {
    if (!loadingLaneIds.value.includes(laneId)) {
      loadingLaneIds.value = [...loadingLaneIds.value, laneId]
    }
    return
  }

  loadingLaneIds.value = loadingLaneIds.value.filter((id) => id !== laneId)
}

function formatWeight(weight: number | null): string {
  if (typeof weight !== 'number' || !Number.isFinite(weight)) {
    return ''
  }

  return `${Math.round(weight)} g`
}

const afcEnabled = computed(() => moonraker.value.afc.available)

const afcObjects = computed(() => {
  return moonraker.value.afc.objects as Record<string, any>
})

const parsedUnits = computed<AfcUnit[]>(() => {
  if (!afcEnabled.value) return []

  const objects = afcObjects.value
  const afcRoot = objects['AFC']

  if (!afcRoot || !Array.isArray(afcRoot.units)) {
    return []
  }

  return afcRoot.units.map((unitName: string) => {
    const unitKey = `AFC_BoxTurtle ${unitName}`
    const unitObject = objects[unitKey]

    const laneNames: string[] = Array.isArray(unitObject?.lanes)
        ? unitObject.lanes
        : Array.isArray(afcRoot.lanes)
            ? afcRoot.lanes
            : []

    const lanes: AfcLane[] = laneNames.map((laneName) => {
      const laneKey = `AFC_stepper ${laneName}`
      const laneObject = objects[laneKey] ?? {}

      const hasFilament = hasLaneFilament(laneObject)
      const isLoaded = isLaneCurrentlyLoaded(laneName, laneObject, afcRoot, objects)

      const laneColor = hasFilament
          ? normalizeColor(laneObject.color)
          : '#434343'

      return {
        id: laneName,
        label: laneName,
        color: laneColor,
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
    })

    return {
      id: unitName,
      label: unitName,
      lanes,
    }
  })
})

const currentUnit = computed(() => {
  if (!props.selectedUnit) return null
  return parsedUnits.value.find((unit) => unit.id === props.selectedUnit) ?? null
})

const currentUnitLaneColors = computed(() => {
  return currentUnit.value?.lanes.map((lane) => lane.color) ?? []
})

function handleEdit(lane: AfcLane) {
  if (lane.isLoaded || isLoading(lane.id) || isEjecting(lane.id)) return
  selectedLaneForDialog.value = { ...lane }
  laneDialogOpen.value = true
}

async function handleEject(lane: AfcLane) {
  if (lane.isLoaded || isEjecting(lane.id) || isLoading(lane.id)) return

  try {
    setLaneEjecting(lane.id, true)

    await moonrakerClient.call('printer.gcode.script', {
      script: `LANE_UNLOAD LANE=${lane.id}`,
    })
  } catch (error) {
    console.error(`Failed to unload lane ${lane.id}`, error)
  } finally {
    setLaneEjecting(lane.id, false)
  }
}

async function handleLoad(lane: AfcLane) {
  if (lane.isLoaded || isLoading(lane.id) || isEjecting(lane.id)) return

  try {
    setLaneLoading(lane.id, true)

    await moonrakerClient.call('printer.gcode.script', {
      script: `TOOL_LOAD LANE=${lane.id}`,
    })
  } catch (error) {
    console.error(`Failed to load lane ${lane.id}`, error)
  } finally {
    setLaneLoading(lane.id, false)
  }
}
</script>

<template>
  <v-card
      v-if="afcEnabled && currentUnit"
      class="afc-card"
      color="transparent"
      elevation="0"
  >
    <v-card-text style="height: 100%; padding: 0">
      <div class="afc-tray">
        <div class="afc-bars">
          <div
              v-for="lane in currentUnit.lanes"
              :key="lane.id"
              class="afc-bar-wrap"
          >
            <div
                class="afc-bar"
                :class="{ 'afc-bar--loaded': lane.isLoaded }"
                :style="{
                backgroundColor: lane.color,
                color: getContrastTextColor(lane.color),
              }"
            >
              <div
                  v-if="lane.hasFilament"
                  class="lane-top-actions"
                  :style="{ background: getActionOverlayColor(lane.color) }"
              >
                <v-btn
                    icon="mdi-pen"
                    variant="plain"
                    :style="{ color: getContrastTextColor(lane.color) }"
                    :disabled="lane.isLoaded || isEjecting(lane.id) || isLoading(lane.id)"
                    @click="handleEdit(lane)"
                />
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
              </div>

              <div
                  v-if="lane.hasFilament"
                  class="lane-actions"
                  :style="{ background: getActionOverlayColor(lane.color) }"
              >
                <v-btn
                    icon="mdi-eject"
                    variant="plain"
                    :style="{ color: getContrastTextColor(lane.color) }"
                    :loading="isEjecting(lane.id)"
                    :disabled="lane.isLoaded || isEjecting(lane.id) || isLoading(lane.id)"
                    @click="handleEject(lane)"
                />
                <v-btn
                    icon="mdi-tray-arrow-down"
                    variant="plain"
                    :style="{ color: getContrastTextColor(lane.color) }"
                    :loading="isLoading(lane.id)"
                    :disabled="lane.isLoaded || isEjecting(lane.id) || isLoading(lane.id)"
                    @click="handleLoad(lane)"
                />
              </div>
            </div>
          </div>
        </div>
      </div>
    </v-card-text>

    <LaneDialogAFC
        v-model="laneDialogOpen"
        :lane="selectedLaneForDialog"
        :lane-colors="currentUnitLaneColors"
    />
  </v-card>

  <v-alert
      v-else
      type="info"
      variant="tonal"
  >
    {{ t('afc.no_unit_selected') }}
  </v-alert>
</template>

<style scoped>
.afc-card {
  height: 100%;
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
  align-items: center;
  justify-content: space-around;
  height: 100%;
  gap: 12px;
}

.afc-bar-wrap {
  display: flex;
  justify-content: center;
  height: 100%;
}

.afc-bar {
  box-shadow: 0px 0px 3px 0px rgb(0 0 0 / 0.33);
  position: relative;
  height: 100%;
  width: 7rem;
  border-radius: 12px;
  border: 2px solid transparent;
  display: flex;
  align-items: center;
  justify-content: center;
}

.afc-bar--loaded {
  border-color: rgb(var(--v-theme-primary));
}

.lane-center {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  min-width: 0;
  max-width: calc(100% - 12px);
  text-align: center;
}

.lane-material {
  font-size: 20px;
  font-weight: 600;
  min-width: 0;
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.lane-weight {
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

.lane-actions {
  border-bottom-left-radius: 10px;
  border-bottom-right-radius: 10px;
  position: absolute;
  bottom: 0;
  width: 100%;
  display: flex;
  align-items: center;
  justify-content: space-around;
}

.lane-top-actions {
  border-top-left-radius: 10px;
  border-top-right-radius: 10px;
  position: absolute;
  top: 0;
  width: 100%;
  display: flex;
  align-items: center;
  justify-content: space-around;
}
</style>