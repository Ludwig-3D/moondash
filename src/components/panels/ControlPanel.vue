<script setup lang="ts">
import { computed, ref } from 'vue'
import { storeToRefs } from 'pinia'
import { useAppStore } from '@/stores/app'
import { moonraker as moonrakerClient } from '@/plugins/moonraker'

const appStore = useAppStore()
const { moonraker } = storeToRefs(appStore)

const runningAction = ref<string | null>(null)
const moveDistance = ref(10)

const rawObjects = computed(() => {
  return moonraker.value.rawObjects as Record<string, unknown>
})

const toolheadObject = computed(() => {
  const value = rawObjects.value.toolhead
  return value && typeof value === 'object' && !Array.isArray(value)
      ? (value as Record<string, unknown>)
      : {}
})

const homedAxes = computed(() => {
  const value = toolheadObject.value.homed_axes
  return typeof value === 'string' ? value.toLowerCase() : ''
})

const isXHomed = computed(() => homedAxes.value.includes('x'))
const isYHomed = computed(() => homedAxes.value.includes('y'))
const isZHomed = computed(() => homedAxes.value.includes('z'))
const isXYHomed = computed(() => isXHomed.value && isYHomed.value)

const hasZTilt = computed(() => 'z_tilt' in rawObjects.value)

const hasQgl = computed(() => {
  return (
      'quad_gantry_level' in rawObjects.value ||
      'gcode_macro QGL' in rawObjects.value ||
      'gcode_macro QUAD_GANTRY_LEVEL' in rawObjects.value
  )
})

const moveDistances = [0.1, 1, 10, 25]

function isBusy(action: string): boolean {
  return runningAction.value === action
}

function setMoveDistance(value: number) {
  moveDistance.value = value
}

async function runGcode(action: string, script: string) {
  if (runningAction.value) return

  try {
    runningAction.value = action
    await moonrakerClient.call('printer.gcode.script', {
      script,
    })
  } catch (error) {
    console.error(`Failed action ${action}`, error)
  } finally {
    runningAction.value = null
  }
}

async function moveXY(x = 0, y = 0) {
  const parts: string[] = []

  if (x !== 0) parts.push(`X${x}`)
  if (y !== 0) parts.push(`Y${y}`)

  if (!parts.length) return
  if ((x !== 0 && !isXHomed.value) || (y !== 0 && !isYHomed.value)) return

  await runGcode(
      `move-xy-${x}-${y}`,
      `G91\nG1 ${parts.join(' ')} F6000\nG90`,
  )
}

async function moveZ(z = 0) {
  if (z === 0) return
  if (!isZHomed.value) return

  await runGcode(
      `move-z-${z}`,
      `G91\nG1 Z${z} F600\nG90`,
  )
}

async function homeXY() {
  await runGcode('home-xy', 'G28 X Y')
}

async function homeZ() {
  await runGcode('home-z', 'G28 Z')
}

async function runZTilt() {
  await runGcode('z-tilt', 'Z_TILT_ADJUST')
}

async function runQgl() {
  if ('gcode_macro QGL' in rawObjects.value) {
    await runGcode('qgl', 'QGL')
    return
  }

  if ('gcode_macro QUAD_GANTRY_LEVEL' in rawObjects.value) {
    await runGcode('qgl', 'QUAD_GANTRY_LEVEL')
    return
  }

  await runGcode('qgl', 'QUAD_GANTRY_LEVEL')
}
</script>

<template>
  <div class="control-panel">
    <div class="control-panel-grid">
      <div class="xy-column">
        <div class="xy-panel">
          <v-btn
              class="move-btn move-btn--up"
              variant="tonal"
              :disabled="!!runningAction || !isYHomed"
              @click="moveXY(0, moveDistance)"
          >
            <v-icon icon="mdi-chevron-up" />
          </v-btn>

          <v-btn
              class="move-btn move-btn--left"
              variant="tonal"
              :disabled="!!runningAction || !isXHomed"
              @click="moveXY(-moveDistance, 0)"
          >
            <v-icon icon="mdi-chevron-left" />
          </v-btn>

          <v-btn
              class="move-btn move-btn--center"
              variant="tonal"
              :loading="isBusy('home-xy')"
              :disabled="!!runningAction"
              @click="homeXY"
          >
            <div class="center-btn-content">
              <v-icon icon="mdi-home" />
              <span>XY</span>
            </div>
          </v-btn>

          <v-btn
              class="move-btn move-btn--right"
              variant="tonal"
              :disabled="!!runningAction || !isXHomed"
              @click="moveXY(moveDistance, 0)"
          >
            <v-icon icon="mdi-chevron-right" />
          </v-btn>

          <v-btn
              class="move-btn move-btn--down"
              variant="tonal"
              :disabled="!!runningAction || !isYHomed"
              @click="moveXY(0, -moveDistance)"
          >
            <v-icon icon="mdi-chevron-down" />
          </v-btn>
        </div>

        <div class="distance-actions">
          <v-btn
              v-for="distance in moveDistances"
              :key="distance"
              class="distance-btn"
              :class="{ 'distance-btn--active': moveDistance === distance }"
              :variant="moveDistance === distance ? 'flat' : 'text'"
              :color="moveDistance === distance ? 'primary' : undefined"
              :disabled="!!runningAction"
              rounded="0"
              @click="setMoveDistance(distance)"
          >
            {{ distance }}
          </v-btn>
        </div>
      </div>

      <div class="z-column">
        <div class="z-panel">
          <v-btn
              class="z-btn"
              variant="tonal"
              :disabled="!!runningAction || !isZHomed"
              @click="moveZ(moveDistance)"
          >
            <div class="z-btn-content">
              <v-icon icon="mdi-chevron-up" />
            </div>
          </v-btn>

          <v-btn
              class="z-btn"
              variant="tonal"
              :loading="isBusy('home-z')"
              :disabled="!!runningAction"
              @click="homeZ"
          >
            <div class="z-btn-content">
              <v-icon icon="mdi-home" />
              <span>Z</span>
            </div>
          </v-btn>

          <v-btn
              class="z-btn"
              variant="tonal"
              :disabled="!!runningAction || !isZHomed"
              @click="moveZ(-moveDistance)"
          >
            <div class="z-btn-content">
              <v-icon icon="mdi-chevron-down" />
            </div>
          </v-btn>
        </div>

        <div
            v-if="hasZTilt || hasQgl"
            class="level-actions"
        >
          <v-btn
              v-if="hasZTilt"
              variant="tonal"
              :loading="isBusy('z-tilt')"
              :disabled="!!runningAction || !isXYHomed"
              @click="runZTilt"
          >
            Z Tilt
          </v-btn>

          <v-btn
              v-if="hasQgl"
              variant="tonal"
              :loading="isBusy('qgl')"
              :disabled="!!runningAction || !isXYHomed"
              @click="runQgl"
          >
            QGL
          </v-btn>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.control-panel {
  height: 100%;
  padding: 16px;
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: 16px;
  border-radius: 5px;
}

.control-panel-grid {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 120px;
  gap: 20px;
  align-items: start;
}

.xy-column,
.z-column {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
}

.xy-panel {
  display: grid;
  grid-template-columns: 72px 72px 72px;
  grid-template-rows: 72px 72px 72px;
  gap: 10px;
  justify-content: center;
}

.move-btn {
  min-width: 72px;
  width: 72px;
  height: 72px;
  padding: 0;
}

.move-btn--up {
  grid-column: 2;
  grid-row: 1;
}

.move-btn--left {
  grid-column: 1;
  grid-row: 2;
}

.move-btn--center {
  grid-column: 2;
  grid-row: 2;
}

.move-btn--right {
  grid-column: 3;
  grid-row: 2;
}

.move-btn--down {
  grid-column: 2;
  grid-row: 3;
}

.center-btn-content,
.z-btn-content {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
  font-size: 0.8rem;
  font-weight: 600;
}

.z-panel {
  display: grid;
  grid-template-rows: repeat(3, 72px);
  gap: 10px;
}

.z-btn {
  min-width: 72px;
  width: 72px;
  height: 72px;
  padding: 0;
}

.distance-actions {
  display: inline-flex;
  align-items: stretch;
  justify-content: center;
  overflow: hidden;
  border-radius: 6px;
  background: rgba(var(--v-theme-on-surface), 0.04);
}

.distance-btn {
  min-width: 64px;
  border-radius: 0 !important;
}

.distance-btn + .distance-btn {
  border-left: 1px solid rgba(var(--v-theme-on-surface), 0.14);
}

.distance-btn--active {
  box-shadow: none !important;
}

.level-actions {
  display: flex;
  justify-content: center;
  flex-wrap: wrap;
  gap: 8px;
  width: 100%;
}

@media (max-width: 700px) {
  .control-panel-grid {
    grid-template-columns: 1fr;
  }

  .z-panel {
    grid-template-columns: repeat(3, 1fr);
    grid-template-rows: none;
  }

  .z-btn {
    width: 100%;
    min-width: 0;
  }
}
</style>