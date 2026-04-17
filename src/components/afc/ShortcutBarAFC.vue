<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { storeToRefs } from 'pinia'
import { useRouter } from 'vue-router'
import { useAppStore } from '@/stores/app'

type AfcLane = {
  id: string
  label: string
  color: string
  loaded: boolean
  map: string
  material: string
}

type AfcUnit = {
  id: string
  label: string
  lanes: AfcLane[]
}

const appStore = useAppStore()
const { moonraker } = storeToRefs(appStore)
const router = useRouter()

const currentUnitIndex = ref(0)
let rotationTimer: ReturnType<typeof setInterval> | null = null

function normalizeColor(color: unknown): string {
  if (typeof color !== 'string' || !color.trim()) {
    return 'rgba(255, 255, 255, 0.16)'
  }
  return color
}

function isLaneLoaded(lane: Record<string, unknown>): boolean {
  return Boolean(
      lane.load ||
      lane.prep ||
      lane.loaded_to_hub ||
      lane.tool_loaded
  )
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

      const loaded = isLaneLoaded(laneObject)
      const laneColor = loaded
          ? normalizeColor(laneObject.color)
          : 'rgba(255, 255, 255, 0.16)'

      return {
        id: laneName,
        label: laneName,
        color: laneColor,
        loaded,
        map: typeof laneObject.map === 'string' ? laneObject.map : '',
        material: typeof laneObject.material === 'string' ? laneObject.material : '',
      }
    })

    return {
      id: unitName,
      label: unitName,
      lanes,
    }
  })
})

const allUnits = computed(() => parsedUnits.value)

const currentUnit = computed(() => {
  if (!allUnits.value.length) return null
  return allUnits.value[currentUnitIndex.value % allUnits.value.length]
})

function stopRotation() {
  if (rotationTimer) {
    clearInterval(rotationTimer)
    rotationTimer = null
  }
}

function startRotation() {
  stopRotation()

  if (allUnits.value.length <= 1) return

  rotationTimer = setInterval(() => {
    currentUnitIndex.value = (currentUnitIndex.value + 1) % allUnits.value.length
  }, 15000)
}

function handleUnitClick() {
  if (!currentUnit.value) return

  router.push({
    path: '/tune',
    query: {
      tab: 'filament',
      afcUnit: currentUnit.value.id,
    },
  })
}

watch(
    allUnits,
    () => {
      currentUnitIndex.value = 0
      startRotation()
    },
    { immediate: true },
)

onMounted(() => {
  startRotation()
})

onBeforeUnmount(() => {
  stopRotation()
})
</script>

<template>
  <v-divider v-if="afcEnabled" />

  <v-btn
      v-if="afcEnabled && currentUnit"
      class="afc-panel-btn"
      variant="text"
      rounded="0"
      block
      @click="handleUnitClick"
  >
    <div class="afc-panel">
      <Transition name="unit-scroll" mode="out-in">
        <div :key="currentUnit.id" class="afc-unit">
          <div class="afc-tray">
            <div class="afc-lanes">
              <div
                  v-for="lane in currentUnit.lanes"
                  :key="lane.id"
                  class="afc-lane"
              >
                <div
                    class="afc-line"
                    :style="{ backgroundColor: lane.color }"
                />
              </div>
            </div>
          </div>
        </div>
      </Transition>
    </div>
  </v-btn>
</template>

<style scoped>
.afc-panel-btn {
  width: 100%;
  min-width: 0;
  height: auto;
  padding: 0;
}

.afc-panel-btn :deep(.v-btn__content) {
  width: 100%;
  display: block;
}

.afc-panel {
  width: 100%;
  padding: 8px 6px;
  box-sizing: border-box;
  overflow: hidden;
}

.afc-unit {
  width: 100%;
}

.afc-tray {
  width: 100%;
  min-height: 26px;
  border-radius: 8px;
  padding: 4px 6px;
  box-sizing: border-box;
}

.afc-lanes {
  display: flex;
  gap: 4px;
  align-items: center;
  justify-content: center;
  min-height: 18px;
}

.afc-lane {
  flex: 1;
  display: flex;
  justify-content: center;
  align-items: center;
}

.afc-line {
  width: 8px;
  height: 18px;
  border-radius: 999px;
  box-shadow:
      inset 0 0 0 1px rgba(255, 255, 255, 0.08),
      0 0 0 1px rgba(0, 0, 0, 0.08);
}

.unit-scroll-enter-active,
.unit-scroll-leave-active {
  transition: opacity 0.28s ease, transform 0.28s ease;
}

.unit-scroll-enter-from {
  opacity: 0;
  transform: translateY(14px);
}

.unit-scroll-leave-to {
  opacity: 0;
  transform: translateY(-14px);
}
</style>