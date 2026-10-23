<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { moonraker as moonrakerClient } from '@/plugins/moonraker'

type Lane = {
  id: string
  label: string
  color: string
  material: string
  weight: number | null
  hasFilament: boolean
  runoutLane: string
}
type Unit = { id: string; label: string; lanes: Lane[] }
const props = defineProps<{ modelValue: boolean; lane: Lane | null; units: Unit[] }>()
const emit = defineEmits<{ (e: 'update:modelValue', value: boolean): void }>()
const { t } = useI18n()
const saving = ref(false)
const errorMessage = ref('')
const destination = ref('NONE')
const selectedUnitId = ref<string | null>(null)
const dialogOpen = computed({
  get: () => props.modelValue,
  set: (value: boolean) => { if (!saving.value) emit('update:modelValue', value) },
})
// Show every lane, including empty ones. Only loaded, non-source lanes are selectable.
const availableUnits = computed(() => props.units)
const currentUnit = computed(() => availableUnits.value.find(unit => unit.id === selectedUnitId.value))
const validDestinations = computed(() => new Set(['NONE', ...availableUnits.value.flatMap(unit => unit.lanes.filter(lane => lane.id !== props.lane?.id && lane.hasFilament).map(lane => lane.id))]))
const isValid = computed(() => validDestinations.value.has(destination.value))

watch(() => [props.modelValue, props.lane?.id, props.lane?.runoutLane] as const, () => {
  if (!props.modelValue) return
  destination.value = props.lane?.runoutLane || 'NONE'
  const unitForDestination = availableUnits.value.find(unit => unit.lanes.some(lane => lane.id === destination.value))
  selectedUnitId.value = unitForDestination?.id ?? availableUnits.value[0]?.id ?? null
  errorMessage.value = ''
}, { immediate: true })

function contrastText(color: string): string {
  const hex = color.replace('#', '')
  if (!/^[0-9a-f]{6}$/i.test(hex)) return '#fff'
  const r = parseInt(hex.slice(0, 2), 16)
  const g = parseInt(hex.slice(2, 4), 16)
  const b = parseInt(hex.slice(4, 6), 16)
  return (r * 299 + g * 587 + b * 114) / 1000 > 160 ? '#111' : '#fff'
}
async function save() {
  if (!props.lane || saving.value || !isValid.value) return
  saving.value = true
  errorMessage.value = ''
  try {
    await moonrakerClient.call('printer.gcode.script', {
      script: `SET_RUNOUT LANE=${props.lane.id} RUNOUT=${destination.value}`,
    })
    emit('update:modelValue', false)
  } catch (error) {
    console.error('Failed to set AFC Infinite Spool', error)
    errorMessage.value = t('afc.infinite_spool.error')
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <v-dialog v-model="dialogOpen" max-width="1000" :persistent="saving">
    <v-card class="spool-dialog" rounded="lg">
      <v-card-title class="spool-header d-flex align-center ga-3">
        <v-icon icon="mdi-infinity" />
        {{ t('afc.infinite_spool.title') }}
        <v-chip
          v-if="lane"
          class="source-lane-chip"
          size="small"
          variant="flat"
          :style="{ backgroundColor: lane.color, color: contrastText(lane.color) }"
        >
          {{ lane.label }}
        </v-chip>
      </v-card-title>
      <v-card-text class="spool-body">
        <div class="spool-section-heading">
          <span class="text-medium-emphasis">{{ t('afc.infinite_spool.destination') }}</span>
          <button
              type="button"
              class="disabled-option"
              :class="{ 'disabled-option--selected': destination === 'NONE' }"
              :aria-pressed="destination === 'NONE'"
              :disabled="saving"
              @click="destination = 'NONE'"
          >
            <v-icon icon="mdi-cancel" size="18" />
            {{ t('afc.infinite_spool.disabled') }}
            <v-icon v-if="destination === 'NONE'" icon="mdi-check" size="18" />
          </button>
        </div>
        <div class="spool-layout">
          <div class="spool-lanes">
            <div v-if="currentUnit" class="afc-bars" :style="{ '--lane-count': currentUnit.lanes.length }">
              <button
                  v-for="candidate in currentUnit.lanes"
                  :key="candidate.id"
                  type="button"
                  class="afc-bar"
                  :class="{ 'afc-bar--selected': destination === candidate.id, 'afc-bar--source': candidate.id === lane?.id, 'afc-bar--empty': !candidate.hasFilament }"
                  :style="{ backgroundColor: candidate.color, color: contrastText(candidate.color) }"
                  :disabled="saving || candidate.id === lane?.id || !candidate.hasFilament"
                  @click="destination = candidate.id"
              >
                <div class="lane-top-info">{{ candidate.label }}</div>
                <v-icon v-if="candidate.id === lane?.id || !candidate.hasFilament" icon="mdi-lock-outline" size="20" class="source-lock" />
                <div class="lane-center">
                  <v-icon v-if="destination === candidate.id" icon="mdi-check-circle" size="24" class="mb-2" />
                  <div class="lane-material">{{ candidate.hasFilament ? candidate.material : "—" }}</div>
                  <div v-if="candidate.weight !== null" class="lane-weight">{{ Math.round(candidate.weight) }} g</div>
                </div>
              </button>
            </div>
            <div v-else class="text-medium-emphasis pa-4">{{ t('afc.no_units') }}</div>
          </div>
          <v-list class="unit-list" density="comfortable" nav variant="tonal" color="primary">
            <v-list-item
                v-for="unit in availableUnits"
                :key="unit.id"
                :active="selectedUnitId === unit.id"
                rounded="lg"
                @click="selectedUnitId = unit.id"
            >{{ unit.label }}</v-list-item>
          </v-list>
        </div>
        <v-alert v-if="errorMessage" class="mt-4" type="error" variant="tonal" :text="errorMessage" />
      </v-card-text>
      <v-card-actions>
        <v-spacer />
        <v-btn  :disabled="saving" @click="dialogOpen = false">
          {{ t('afc.infinite_spool.cancel') }}
        </v-btn>
        <v-btn variant="tonal" color="primary" :loading="saving" :disabled="!lane || !isValid" @click="save">
          {{ t('afc.infinite_spool.save') }}
        </v-btn>
      </v-card-actions>
    </v-card>
  </v-dialog>
</template>

<style scoped>
.spool-dialog {
  display: flex;
  flex-direction: column;
  overflow: hidden;
  max-height: none;
}
.spool-header {
  flex: 0 0 auto;
  padding: 18px 24px 8px !important;
  line-height: 1.3;
}
.spool-body {
  padding: 8px 24px 12px !important;
  min-height: 0;
  overflow: visible;
}
.source-lane-chip { font-weight: 600; flex-shrink: 0; }
.spool-section-heading { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-bottom: 10px; flex-wrap: wrap; }
.spool-layout { display: grid; grid-template-columns: minmax(0, 1fr) 140px; gap: 12px; min-height: 0; }
.spool-lanes { min-width: 0; overflow: visible; padding: 4px; }
.afc-bars { display: grid; grid-template-columns: repeat(var(--lane-count, 4), minmax(0, 1fr)); align-items: stretch; gap: 10px; min-height: 240px; }
.afc-bar { appearance: none; min-width: 0; width: 100%; min-height: 240px; position: relative; border-radius: 12px; border: 2px solid transparent; box-shadow: 0 0 3px #0005; cursor: pointer; overflow: hidden; display: flex; align-items: center; justify-content: center; transition: transform 120ms ease, border-color 120ms ease; }
.afc-bar:hover:not(:disabled) { transform: translateY(-2px); }
.afc-bar--selected { border-color: rgb(var(--v-theme-primary)); box-shadow: 0 0 0 2px rgba(var(--v-theme-primary), .3); }
.afc-bar--source, .afc-bar--empty { filter: saturate(.45); }
.afc-bar:disabled { cursor: not-allowed; }
.afc-bar--source:disabled, .afc-bar--empty:disabled { opacity: .5; }
.source-lock { position: absolute; top: 38px; right: 8px; }
.lane-top-info { position: absolute; top: 0; width: 100%; min-height: 30px; display: flex; align-items: center; justify-content: center; font-size: .8rem; font-weight: 700; background: #0003; }
.lane-center { text-align: center; min-width: 0; padding: 20px 4px 0; }
.lane-material { font-size: 17px; font-weight: 600; overflow-wrap: anywhere; }
.lane-weight { font-size: 12px; margin-top: 4px; }
.unit-list { min-width: 0; border-radius: 12px; background: rgba(var(--v-theme-on-surface), .08); }
.disabled-option { display: inline-flex; align-items: center; justify-content: center; gap: 8px; padding: 7px 12px; min-height: 40px; background: rgba(var(--v-theme-on-surface), .06); border: 1px solid rgba(var(--v-theme-on-surface), .18); border-radius: 10px; cursor: pointer; font: inherit; font-size: .875rem; transition: background 120ms ease, border-color 120ms ease; }
.disabled-option:hover:not(:disabled) { background: rgba(var(--v-theme-on-surface), .12); }
.disabled-option--selected { border-color: rgb(var(--v-theme-primary)); color: rgb(var(--v-theme-primary)); background: rgba(var(--v-theme-primary), .1); }
.disabled-option:disabled { opacity: .6; }
@media (max-width: 600px) {
  .spool-header { padding: 12px 16px 6px !important; }
  .spool-body { padding: 6px 16px 8px !important; }
  .spool-layout { grid-template-columns: 1fr; }
  .unit-list { display: flex; flex-wrap: wrap; }
  .afc-bars { min-height: 180px; }
  .afc-bar { min-height: 180px; }
}
</style>
