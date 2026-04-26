<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useAppStore } from '@/stores/app'
import ColorPickerDialog from '@/components/dialogs/ColorPickerDialog.vue'

const { t } = useI18n()
const appStore = useAppStore()

const saving = ref(false)
const suppressAutoSave = ref(false)

const localDarkmode = ref(true)
const localPrimary = ref('')
const localSecondary = ref('')
const localLanguage = ref<string | null>(null)
const localUseIdleTimeout = ref(true)
const localIdleTimeout = ref(360)

const primaryPickerOpen = ref(false)
const secondaryPickerOpen = ref(false)

let saveTimeout: ReturnType<typeof setTimeout> | null = null

const languageItems = [
  { title: 'System Default', value: null },
  { title: 'English', value: 'en' },
  { title: 'Deutsch', value: 'de' },
]

const idleTimeoutItems = computed(() => [
  { title: t('settings.config.idle_timeout_options.1_minute'), value: 60 },
  { title: t('settings.config.idle_timeout_options.5_minutes'), value: 300 },
  { title: t('settings.config.idle_timeout_options.15_minutes'), value: 900 },
  { title: t('settings.config.idle_timeout_options.30_minutes'), value: 1800 },
  { title: t('settings.config.idle_timeout_options.1_hour'), value: 3600 },
])

const defaultLightPrimary = '#1867C0'
const defaultLightSecondary = '#48A9A6'
const defaultDarkPrimary = '#2196F3'
const defaultDarkSecondary = '#54B6B2'

const vanillaPrimary = computed(() => {
  return localDarkmode.value ? defaultDarkPrimary : defaultLightPrimary
})

const vanillaSecondary = computed(() => {
  return localDarkmode.value ? defaultDarkSecondary : defaultLightSecondary
})

const currentPrimaryPreview = computed(() => localPrimary.value.trim() || vanillaPrimary.value)
const currentSecondaryPreview = computed(() => localSecondary.value.trim() || vanillaSecondary.value)

watch(
    () => [
      appStore.isDarkmode,
      appStore.getPrimaryColor,
      appStore.getSecondaryColor,
      appStore.getLanguage,
      appStore.getUseIdleTimeout,
      appStore.getIdleTimeout,
    ],
    () => {
      suppressAutoSave.value = true

      localDarkmode.value = appStore.isDarkmode
      localPrimary.value = appStore.getPrimaryColor ?? ''
      localSecondary.value = appStore.getSecondaryColor ?? ''
      localLanguage.value = appStore.getLanguage
      localUseIdleTimeout.value = appStore.getUseIdleTimeout
      localIdleTimeout.value = appStore.getIdleTimeout ?? 360

      queueMicrotask(() => {
        suppressAutoSave.value = false
      })
    },
    { immediate: true },
)

function normalizePrimaryColorInput(value: string): string {
  const trimmed = value.trim()
  return trimmed || vanillaPrimary.value
}

function normalizeSecondaryColorInput(value: string): string {
  const trimmed = value.trim()
  return trimmed || vanillaSecondary.value
}

function onPrimarySelected(color: string) {
  localPrimary.value = color
}

function onSecondarySelected(color: string) {
  localSecondary.value = color
}

async function saveConfig() {
  if (saving.value) return

  try {
    saving.value = true

    await appStore.saveEditableConfig({
      styling: {
        darkmode: localDarkmode.value,
        primary: normalizePrimaryColorInput(localPrimary.value),
        secondary: normalizeSecondaryColorInput(localSecondary.value),
      },
      system: {
        language: localLanguage.value,
        use_idle_timeout: localUseIdleTimeout.value,
        idle_timeout: localIdleTimeout.value,
      },
    })
  } finally {
    saving.value = false
  }
}

function scheduleSave() {
  if (suppressAutoSave.value) return

  if (saveTimeout) {
    clearTimeout(saveTimeout)
  }

  saveTimeout = setTimeout(() => {
    saveTimeout = null
    void saveConfig()
  }, 250)
}

watch(localDarkmode, scheduleSave)
watch(localPrimary, scheduleSave)
watch(localSecondary, scheduleSave)
watch(localLanguage, scheduleSave)
watch(localUseIdleTimeout, scheduleSave)
watch(localIdleTimeout, scheduleSave)
</script>

<template>
  <v-card rounded="lg" variant="flat" class="config-editor-panel" height="100%">
    <v-card-text>
      <div class="config-editor-grid">
        <div class="config-editor-label">
          {{ t('settings.config.darkmode') }}
        </div>
        <div>
          <v-switch
              v-model="localDarkmode"
              color="primary"
              hide-details
              inset
              density="compact"
              :disabled="saving"
          />
        </div>

        <div class="config-editor-label">
          {{ t('settings.config.colors') }}
        </div>
        <div class="config-editor-color-row">
          <button
              type="button"
              class="config-editor-color-preview"
              :style="{ backgroundColor: currentPrimaryPreview }"
              :disabled="saving"
              @click="primaryPickerOpen = true"
          />
          <button
              type="button"
              class="config-editor-color-preview"
              :style="{ backgroundColor: currentSecondaryPreview }"
              :disabled="saving"
              @click="secondaryPickerOpen = true"
          />
        </div>

        <div class="config-editor-label">
          {{ t('settings.config.language') }}
        </div>
        <div>
          <v-select
              v-model="localLanguage"
              :items="languageItems"
              item-title="title"
              item-value="value"
              variant="outlined"
              density="comfortable"
              hide-details
              :disabled="saving"
          />
        </div>

        <div class="config-editor-label">
          {{ t('settings.config.use_idle_timeout') }}
        </div>
        <div>
          <v-switch
              v-model="localUseIdleTimeout"
              color="primary"
              hide-details
              inset
              density="compact"
              :disabled="saving"
          />
        </div>

        <template v-if="localUseIdleTimeout">
          <div class="config-editor-label">
            {{ t('settings.config.idle_timeout') }}
          </div>
          <div>
            <v-select
                v-model="localIdleTimeout"
                :items="idleTimeoutItems"
                item-title="title"
                item-value="value"
                variant="outlined"
                density="comfortable"
                hide-details
                :disabled="saving"
            />
          </div>
        </template>
      </div>
    </v-card-text>

    <v-card-actions>
      <v-spacer />
      <v-progress-circular
          v-if="saving"
          indeterminate
          size="20"
          width="2"
          color="primary"
      />
    </v-card-actions>
  </v-card>

  <ColorPickerDialog
      v-model="primaryPickerOpen"
      :selected-color="currentPrimaryPreview"
      :additional-colors="[vanillaPrimary, vanillaSecondary]"
      :additional-colors-title="t('settings.config.default_colors')"
      :vibrant-only="true"
      @select="onPrimarySelected"
  />

  <ColorPickerDialog
      v-model="secondaryPickerOpen"
      :selected-color="currentSecondaryPreview"
      :additional-colors="[vanillaPrimary, vanillaSecondary]"
      :additional-colors-title="t('settings.config.default_colors')"
      :vibrant-only="true"
      @select="onSecondarySelected"
  />
</template>

<style scoped>
.config-editor-panel {
  width: 100%;
}

.config-editor-grid {
  display: grid;
  grid-template-columns: 320px minmax(0, 1fr);
  gap: 18px 20px;
  align-items: center;
}

.config-editor-label {
  font-weight: 600;
}

.config-editor-color-row {
  display: flex;
  align-items: center;
  gap: 12px;
  min-width: 0;
}

.config-editor-color-preview {
  width: 42px;
  height: 42px;
  border-radius: 10px;
  border: 2px solid rgb(0 0 0 / 0.25);
  flex: 0 0 auto;
  cursor: pointer;
}

.config-editor-color-preview:disabled {
  opacity: 0.7;
  cursor: default;
}
</style>