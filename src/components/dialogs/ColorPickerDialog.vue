<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

const props = withDefaults(defineProps<{
  modelValue: boolean
  selectedColor: string
  additionalColors?: string[]
  additionalColorsTitle?: string | null
  vibrantOnly?: boolean
}>(), {
  additionalColors: () => [],
  additionalColorsTitle: null,
  vibrantOnly: false,
})

const emit = defineEmits<{
  (e: 'update:modelValue', value: boolean): void
  (e: 'select', value: string): void
}>()

const dialogOpen = computed({
  get: () => props.modelValue,
  set: (value: boolean) => emit('update:modelValue', value),
})

const allColorOptions = [
  '#000000',
  '#FFFFFF',
  '#1A1818',
  '#434343',
  '#6D4C41',
  '#8D6E63',

  '#E53935',
  '#F44336',
  '#F4511E',
  '#FF7043',
  '#E66100',
  '#FB8C00',

  '#FFB74D',
  '#FDD835',
  '#FFF176',
  '#C0CA33',
  '#9CCC65',
  '#7CB342',

  '#43A047',
  '#66BB6A',
  '#00897B',
  '#26A69A',
  '#00ACC1',
  '#4DD0E1',

  '#039BE5',
  '#64B5F6',
  '#1E88E5',
  '#3949AB',
  '#5E35B1',
  '#9575CD',

  '#8E24AA',
  '#D81B60',
  '#F06292',
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

const filteredBaseColors = computed(() => {
  if (!props.vibrantOnly) return allColorOptions

  return allColorOptions.filter((color) => {
    const normalized = normalizeHexColor(color)
    return !['#000000', '#FFFFFF', '#1A1818', '#434343'].includes(normalized)
  })
})

const normalizedSelectedColor = computed(() => normalizeHexColor(props.selectedColor))

const additionalColors = computed(() => {
  const seen = new Set<string>()
  const result: string[] = []

  for (const color of props.additionalColors ?? []) {
    const normalized = normalizeHexColor(color)

    if (normalized === '#434343') continue
    if (props.vibrantOnly && ['#000000', '#FFFFFF', '#1A1818', '#434343'].includes(normalized)) {
      continue
    }
    if (normalized === normalizedSelectedColor.value) continue
    if (seen.has(normalized)) continue

    seen.add(normalized)
    result.push(normalized)
  }

  return result
})

function closeDialog() {
  dialogOpen.value = false
}

function selectColor(color: string) {
  emit('select', normalizeHexColor(color))
  dialogOpen.value = false
}
</script>

<template>
  <v-dialog
      v-model="dialogOpen"
      fullscreen
      persistent
      transition="dialog-bottom-transition"
  >
    <v-card>
      <v-toolbar>
        <v-btn
            icon="mdi-close"
            @click="closeDialog"
        />
        <v-toolbar-title>{{ t('color_dialog.choose_color') }}</v-toolbar-title>
        <v-spacer />
      </v-toolbar>

      <v-card-text class="color-picker-fullscreen">
        <div
            v-if="additionalColors.length"
            class="color-picker-section"
        >
          <div class="color-picker-section-title">
            {{ additionalColorsTitle || t('color_dialog.additional_colors') }}
          </div>

          <div class="color-picker-swatches color-picker-swatches--compact">
            <button
                v-for="color in additionalColors"
                :key="`additional-${color}`"
                type="button"
                class="color-picker-swatch color-picker-swatch--small"
                :class="{ 'color-picker-swatch--active': normalizedSelectedColor === color }"
                :style="{ backgroundColor: color }"
                @click="selectColor(color)"
            />
          </div>
        </div>

        <div class="color-picker-section">
          <div class="color-picker-section-title" v-if="additionalColorsTitle">
            {{ t('color_dialog.other_colors') }}
          </div>

          <div class="color-picker-swatches">
            <button
                v-for="color in filteredBaseColors"
                :key="color"
                type="button"
                class="color-picker-swatch"
                :class="{ 'color-picker-swatch--active': normalizedSelectedColor === color }"
                :style="{ backgroundColor: color }"
                @click="selectColor(color)"
            />
          </div>
        </div>
      </v-card-text>
    </v-card>
  </v-dialog>
</template>

<style scoped>
.color-picker-fullscreen {
  padding: 24px;
}

.color-picker-section + .color-picker-section {
  margin-top: 20px;
}

.color-picker-section-title {
  font-size: 1.1rem;
  margin-bottom: 12px;
  opacity: 0.9;
}

.color-picker-swatches {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(48px, 1fr));
  gap: 18px;
}

.color-picker-swatches--compact {
  grid-template-columns: repeat(auto-fill, minmax(48px, 1fr));
  gap: 14px;
}

.color-picker-swatch {
  width: 100%;
  aspect-ratio: 1;
  border-radius: 18px;
  border: 2px solid rgb(0 0 0 / 0.35);
  cursor: pointer;
}

.color-picker-swatch--small {
  border-radius: 14px;
}

.color-picker-swatch--active {
  outline: 4px solid rgb(var(--v-theme-primary));
  outline-offset: 3px;
}
</style>