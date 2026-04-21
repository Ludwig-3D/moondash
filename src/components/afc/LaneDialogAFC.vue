<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { moonraker as moonrakerClient } from '@/plugins/moonraker'
import { useI18n } from 'vue-i18n'
import ColorPickerDialog from '@/components/dialogs/ColorPickerDialog.vue'

const { t } = useI18n()

type AfcLaneDialogData = {
  id: string
  label: string
  color: string
  material: string
  weight: number | null
}

const props = defineProps<{
  modelValue: boolean
  lane: AfcLaneDialogData | null
  laneColors?: string[]
}>()

const emit = defineEmits<{
  (e: 'update:modelValue', value: boolean): void
}>()

const dialogOpen = computed({
  get: () => props.modelValue,
  set: (value: boolean) => {
    if (saving.value) return
    emit('update:modelValue', value)
  },
})

const colorPickerOpen = ref(false)
const saving = ref(false)
const localMaterial = ref('')
const localColor = ref('#434343')
const localWeight = ref<string>('')

const materialOptions = [
  'ABS',
  'ASA',
  'PETG',
  'PLA',
  'TPU',
  'PA',
  'PC',
]

function normalizeHexColor(color: unknown): string {
  if (typeof color !== 'string' || !color.trim()) {
    return '#434343'
  }

  const value = color.trim()

  if (/^#[0-9a-f]{6}$/i.test(value)) {
    return value.toUpperCase()
  }

  if (/^#[0-9a-f]{3}$/i.test(value)) {
    return value.toUpperCase()
  }

  return '#434343'
}

watch(
    () => props.lane,
    (lane) => {
      localMaterial.value = lane?.material?.trim() || ''
      localColor.value = normalizeHexColor(lane?.color)
      localWeight.value =
          typeof lane?.weight === 'number' && Number.isFinite(lane.weight)
              ? String(Math.round(lane.weight))
              : ''
      colorPickerOpen.value = false
    },
    { immediate: true },
)

function closeDialog() {
  if (saving.value) return
  dialogOpen.value = false
  colorPickerOpen.value = false
}

function openColorPicker() {
  if (saving.value) return
  colorPickerOpen.value = true
}

function parseWeight(value: string): number | null {
  const trimmed = value.trim()
  if (!trimmed) return null

  const parsed = Number(trimmed)
  if (!Number.isFinite(parsed)) return null

  return Math.round(parsed)
}

function stripHash(color: string): string {
  return color.replace(/^#/, '').toUpperCase()
}

function adjustWeight(delta: number) {
  if (saving.value) return

  const current = parseWeight(localWeight.value) ?? 0
  const next = Math.max(0, current + delta)
  localWeight.value = String(next)
}

function onColorSelected(color: string) {
  localColor.value = normalizeHexColor(color)
}

async function saveDialog() {
  if (!props.lane || saving.value) return

  const laneId = props.lane.id
  const material = localMaterial.value.trim()
  const color = stripHash(localColor.value)
  const weight = parseWeight(localWeight.value)

  let saveSucceeded = false

  try {
    saving.value = true

    if (material) {
      await moonrakerClient.call('printer.gcode.script', {
        script: `SET_MATERIAL LANE=${laneId} MATERIAL=${material}`,
      })
    }

    await moonrakerClient.call('printer.gcode.script', {
      script: `SET_COLOR LANE=${laneId} COLOR=${color}`,
    })

    if (weight !== null) {
      await moonrakerClient.call('printer.gcode.script', {
        script: `SET_WEIGHT LANE=${laneId} WEIGHT=${weight}`,
      })
    }

    saveSucceeded = true
  } catch (error) {
    console.error(`Failed to save lane ${laneId}`, error)
  } finally {
    saving.value = false
  }

  if (saveSucceeded) {
    colorPickerOpen.value = false
    emit('update:modelValue', false)
  }
}
</script>

<template>
  <v-dialog v-model="dialogOpen" max-width="760" persistent>
    <v-card rounded="lg">
      <v-card-title class="text-h5 pt-6 px-6">
        {{ lane?.label ?? t('afc.edit.lane') }}
      </v-card-title>

      <v-card-text class="px-6 pb-2">
        <div class="lane-dialog-grid">
          <div class="lane-dialog-label">
            {{ t('afc.edit.filament') }}
          </div>

          <div class="lane-dialog-field">
            <v-select
                v-model="localMaterial"
                :items="materialOptions"
                variant="outlined"
                density="comfortable"
                hide-details
                :placeholder="t('afc.edit.select_material')"
                :disabled="saving"
            />
          </div>

          <div class="lane-dialog-label">
            {{ t('afc.edit.color') }}
          </div>

          <div class="lane-dialog-color-row">
            <button
                class="lane-dialog-color-preview lane-dialog-color-preview--button"
                :style="{ backgroundColor: localColor }"
                type="button"
                :disabled="saving"
                @click="openColorPicker"
            />

            <div class="lane-dialog-color-value">
              {{ localColor }}
            </div>
          </div>

          <div class="lane-dialog-label">
            {{ t('afc.edit.weight') }}
          </div>

          <div class="lane-dialog-weight-wrap">
            <div class="lane-dialog-weight-adjust-row">
              <button
                  type="button"
                  class="lane-dialog-weight-adjust"
                  :disabled="saving"
                  @click="adjustWeight(-100)"
              >
                -100
              </button>
              <button
                  type="button"
                  class="lane-dialog-weight-adjust"
                  :disabled="saving"
                  @click="adjustWeight(-10)"
              >
                -10
              </button>
              <button
                  type="button"
                  class="lane-dialog-weight-adjust"
                  :disabled="saving"
                  @click="adjustWeight(-1)"
              >
                -1
              </button>

              <div class="lane-dialog-weight-adjust-value">
                <v-text-field
                    v-model="localWeight"
                    variant="outlined"
                    density="comfortable"
                    hide-details
                    type="number"
                    min="0"
                    step="1"
                    suffix="g"
                    :placeholder="t('afc.edit.weight_placeholder')"
                    :disabled="saving"
                />
              </div>

              <button
                  type="button"
                  class="lane-dialog-weight-adjust"
                  :disabled="saving"
                  @click="adjustWeight(1)"
              >
                +1
              </button>
              <button
                  type="button"
                  class="lane-dialog-weight-adjust"
                  :disabled="saving"
                  @click="adjustWeight(10)"
              >
                +10
              </button>
              <button
                  type="button"
                  class="lane-dialog-weight-adjust"
                  :disabled="saving"
                  @click="adjustWeight(100)"
              >
                +100
              </button>
            </div>
          </div>
        </div>
      </v-card-text>

      <v-card-actions class="px-6 pb-6">
        <v-spacer />
        <v-btn variant="text" :disabled="saving" @click="closeDialog">
          {{ t('afc.edit.cancel') }}
        </v-btn>
        <v-btn
            color="primary"
            variant="flat"
            :loading="saving"
            :disabled="saving"
            @click="saveDialog"
        >
          {{ t('afc.edit.save') }}
        </v-btn>
      </v-card-actions>
    </v-card>
  </v-dialog>

  <ColorPickerDialog
      v-model="colorPickerOpen"
      :selected-color="localColor"
      :additional-colors="props.laneColors ?? []"
      :additional-colors-title="t('afc.edit.other_lane_colors')"
      :vibrant-only="false"
      @select="onColorSelected"
  />
</template>

<style scoped>
.lane-dialog-grid {
  display: grid;
  grid-template-columns: 120px minmax(0, 1fr);
  gap: 24px 24px;
  align-items: center;
}

.lane-dialog-label {
  font-size: 1.25rem;
}

.lane-dialog-field {
  min-width: 0;
}

.lane-dialog-color-row {
  display: flex;
  align-items: center;
  gap: 16px;
}

.lane-dialog-color-preview {
  width: 48px;
  height: 48px;
  border-radius: 8px;
  border: 2px solid rgb(0 0 0 / 0.35);
  flex: 0 0 auto;
}

.lane-dialog-color-preview--button {
  cursor: pointer;
}

.lane-dialog-color-preview--button:disabled {
  cursor: default;
  opacity: 0.7;
}

.lane-dialog-color-value {
  font-weight: 300;
  letter-spacing: 0.04em;
}

.lane-dialog-weight-wrap {
  min-width: 0;
}

.lane-dialog-weight-adjust-row {
  display: grid;
  grid-template-columns: repeat(3, 60px) minmax(140px, 1fr) repeat(3, 60px);
  align-items: stretch;
  border: 1px solid rgba(255, 255, 255, 0.14);
  border-radius: 10px;
  overflow: hidden;
}

.lane-dialog-weight-adjust {
  appearance: none;
  border: 0;
  border-right: 1px solid rgba(255, 255, 255, 0.14);
  background: rgba(255, 255, 255, 0.02);
  color: inherit;
  font: inherit;
  padding: 0 8px;
  min-height: 44px;
  cursor: pointer;
}

.lane-dialog-weight-adjust:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.06);
}

.lane-dialog-weight-adjust:disabled {
  opacity: 0.5;
  cursor: default;
}

.lane-dialog-weight-adjust-value {
  min-width: 0;
  border-right: 1px solid rgba(255, 255, 255, 0.14);
  display: flex;
  align-items: stretch;
}

.lane-dialog-weight-adjust-value :deep(.v-input) {
  width: 100%;
}

.lane-dialog-weight-adjust-value :deep(.v-field) {
  border-radius: 0;
  box-shadow: none;
}
</style>