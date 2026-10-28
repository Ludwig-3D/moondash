<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()
const props = defineProps<{
  modelValue: boolean
  running: boolean
  application: string
  messages: string[]
  error: string
}>()
const emit = defineEmits<{ (e: 'update:modelValue', value: boolean): void }>()
const dialogOpen = computed({
  get: () => props.modelValue,
  set: (value: boolean) => emit('update:modelValue', value),
})
const logElement = ref<HTMLElement | null>(null)
watch(() => props.messages.length, async () => {
  if (!props.modelValue) return
  await nextTick()
  if (logElement.value) logElement.value.scrollTop = logElement.value.scrollHeight
})
</script>

<template>
  <v-dialog v-model="dialogOpen" max-width="680" persistent :scrim="true" scrollable>
    <v-card class="update-progress-dialog" rounded="lg">
      <v-card-title class="update-progress-dialog__header d-flex align-center ga-3">
        <v-icon icon="mdi-update" />
        {{ t('settings.updates.dialog.title') }}
        <v-spacer />
        <v-progress-circular v-if="running" size="22" width="2" indeterminate />
        <v-icon v-else :icon="error ? 'mdi-alert-circle-outline' : 'mdi-check-circle-outline'"
                :color="error ? 'error' : 'success'" />
      </v-card-title>

      <v-card-text class="update-progress-dialog__content">
        <v-progress-linear v-if="running" indeterminate rounded class="mb-4" />
        <div v-if="application" class="text-subtitle-2 mb-2">{{ application }}</div>
        <div ref="logElement" class="update-progress-dialog__log" role="log" aria-live="polite">
          <div v-for="(line, index) in messages" :key="index">{{ line }}</div>
          <div v-if="!messages.length" class="text-medium-emphasis">
            {{ t('settings.updates.dialog.preparing') }}
          </div>
        </div>
        <v-alert v-if="error" type="error" variant="tonal" class="mt-3">{{ error }}</v-alert>
      </v-card-text>

      <v-card-actions class="update-progress-dialog__footer">
        <v-spacer />
        <v-btn :disabled="running" variant="text" @click="dialogOpen = false">
          {{ t('settings.updates.dialog.close') }}
        </v-btn>
      </v-card-actions>
    </v-card>
  </v-dialog>
</template>

<style scoped>
.update-progress-dialog {
  display: flex;
  flex-direction: column;
  max-height: min(620px, calc(100dvh - 48px));
  overflow: hidden;
}
.update-progress-dialog__header,
.update-progress-dialog__footer { flex: 0 0 auto; }
.update-progress-dialog__content {
  display: flex;
  flex-direction: column;
  flex: 1 1 auto;
  min-height: 0;
  overflow: hidden;
}
.update-progress-dialog__log {
  flex: 1 1 auto;
  min-height: 120px;
  max-height: 340px;
  overflow-y: auto;
  overflow-x: hidden;
  padding: 12px;
  border-radius: 8px;
  background: rgba(var(--v-theme-on-surface), .05);
  font-family: monospace;
  font-size: 12px;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
</style>
