<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { moonraker as moonrakerClient } from '@/plugins/moonraker'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

type FanDialogData = {
  key: string
  label: string
  speed: number | null
  temperature: number | null
  target: number | null
  isTemperatureFan: boolean
  maxValue?: number | null
}

const props = defineProps<{
  modelValue: boolean
  fan: FanDialogData | null
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

const saving = ref(false)
const localValue = ref('')

const resolvedMax = computed(() => {
  if (typeof props.fan?.maxValue === 'number' && Number.isFinite(props.fan.maxValue)) {
    return Math.max(0, Math.round(props.fan.maxValue))
  }

  return props.fan?.isTemperatureFan ? 120 : 100
})

watch(
    () => props.fan,
    (fan) => {
      if (!fan) {
        localValue.value = ''
        return
      }

      if (fan.isTemperatureFan) {
        localValue.value =
            typeof fan.target === 'number' && Number.isFinite(fan.target)
                ? String(Math.round(fan.target))
                : ''
        return
      }

      localValue.value =
          typeof fan.speed === 'number' && Number.isFinite(fan.speed)
              ? String(Math.round(fan.speed * 100))
              : ''
    },
    { immediate: true },
)

function closeDialog() {
  if (saving.value) return
  dialogOpen.value = false
}

function parseValue(value: string): number | null {
  const trimmed = value.trim()
  if (!trimmed) return null

  const parsed = Number(trimmed)
  if (!Number.isFinite(parsed)) return null

  return Math.round(parsed)
}

function clampValue(value: number): number {
  return Math.max(0, Math.min(resolvedMax.value, Math.round(value)))
}

function adjustValue(delta: number) {
  if (saving.value) return

  const current = parseValue(localValue.value) ?? 0
  localValue.value = String(clampValue(current + delta))
}

function setMin() {
  if (saving.value) return
  localValue.value = '0'
}

function setMax() {
  if (saving.value) return
  localValue.value = String(resolvedMax.value)
}

function getTemperatureFanName(key: string): string {
  return key.replace(/^temperature_fan\s+/i, '')
}

function getFanName(key: string): string | null {
  if (key.toLowerCase() === 'fan') return null

  return key
      .replace(/^fan_generic\s+/i, '')
      .replace(/^heater_fan\s+/i, '')
      .replace(/^controller_fan\s+/i, '')
      .trim()
}

async function saveDialog() {
  if (!props.fan || saving.value) return

  const value = clampValue(parseValue(localValue.value) ?? 0)
  const fanKey = props.fan.key

  try {
    saving.value = true

    if (props.fan.isTemperatureFan) {
      const temperatureFanName = getTemperatureFanName(fanKey)

      await moonrakerClient.call('printer.gcode.script', {
        script: `SET_TEMPERATURE_FAN_TARGET TEMPERATURE_FAN=${temperatureFanName} TARGET=${value}`,
      })
    } else {
      const fanName = getFanName(fanKey)
      const speedValue = (value / 100).toFixed(2)
      const pwm255 = Math.round((value / 100) * 255)

      const script = fanName
          ? `SET_FAN_SPEED FAN=${fanName} SPEED=${speedValue}`
          : `M106 S${pwm255}`

      await moonrakerClient.call('printer.gcode.script', {
        script,
      })
    }

    emit('update:modelValue', false)
  } catch (error) {
    console.error(`Failed to save fan ${fanKey}`, error)
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <v-dialog v-model="dialogOpen" max-width="760" persistent>
    <v-card rounded="lg">
      <v-card-title class="text-h5 pt-6 px-6">
        {{ fan?.label ?? t('fan.dialog.title') }}
      </v-card-title>

      <v-card-text class="px-6 pb-2">
        <div class="form-grid">
          <div class="form-label">
            {{ t('fan.dialog.target') }}
          </div>

          <div class="form-field">
            <div class="control-row control-row--compact">
              <button
                  type="button"
                  class="control-btn"
                  :disabled="saving"
                  @click="setMin"
              >
                min
              </button>

              <button
                  type="button"
                  class="control-btn"
                  :disabled="saving"
                  @click="adjustValue(-10)"
              >
                -10
              </button>

              <button
                  type="button"
                  class="control-btn"
                  :disabled="saving"
                  @click="adjustValue(-1)"
              >
                -1
              </button>

              <div class="control-input">
                <v-text-field
                    v-model="localValue"
                    variant="outlined"
                    density="comfortable"
                    hide-details
                    type="number"
                    min="0"
                    :max="resolvedMax"
                    step="1"
                    :suffix="fan?.isTemperatureFan ? '°C' : '%'"
                    :placeholder="fan?.isTemperatureFan ? t('fan.dialog.temp_placeholder') : t('fan.dialog.speed_placeholder')"
                    :disabled="saving"
                />
              </div>

              <button
                  type="button"
                  class="control-btn"
                  :disabled="saving"
                  @click="adjustValue(1)"
              >
                +1
              </button>

              <button
                  type="button"
                  class="control-btn"
                  :disabled="saving"
                  @click="adjustValue(10)"
              >
                +10
              </button>

              <button
                  type="button"
                  class="control-btn"
                  :disabled="saving"
                  @click="setMax"
              >
                max
              </button>
            </div>
          </div>
        </div>
      </v-card-text>

      <v-card-actions>
        <v-spacer />
        <v-btn variant="text" :disabled="saving" @click="closeDialog">
          {{ t('fan.dialog.cancel') }}
        </v-btn>
        <v-btn
            color="primary"
            :loading="saving"
            :disabled="saving"
            @click="saveDialog"
        >
          {{ t('fan.dialog.save') }}
        </v-btn>
      </v-card-actions>
    </v-card>
  </v-dialog>
</template>

<style scoped>
</style>