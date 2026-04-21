<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { moonraker as moonrakerClient } from '@/plugins/moonraker'

const { t } = useI18n()

const props = defineProps<{
  titleKey: string
  message: string
}>()

const restarting = ref(false)
const firmwareRestarting = ref(false)

const alertType = computed(() => {
  return props.titleKey === 'notifications.klipper.title_startup' ? 'info' : 'error'
})

async function restartKlipper() {
  if (restarting.value || firmwareRestarting.value) return

  try {
    restarting.value = true
    await moonrakerClient.call('printer.restart')
  } finally {
    restarting.value = false
  }
}

async function firmwareRestartKlipper() {
  if (restarting.value || firmwareRestarting.value) return

  try {
    firmwareRestarting.value = true
    await moonrakerClient.call('printer.firmware_restart')
  } finally {
    firmwareRestarting.value = false
  }
}
</script>

<template>
  <v-alert
      :type="alertType"
      variant="tonal"
      border="start"
      density="compact"
  >
    <div class="alert-body">
      <div class="alert-body__content">
        <div class="alert-body__title">{{ t(props.titleKey) }}</div>
        <div class="alert-body__message">
          {{ props.message || t('notifications.klipper.message_fallback') }}
        </div>
      </div>

      <div class="alert-body__actions">
        <v-btn
            variant="text"
            :loading="restarting"
            :disabled="restarting || firmwareRestarting"
            @click="restartKlipper"
        >
          {{ t('notifications.klipper.restart') }}
        </v-btn>

        <v-btn
            :color="alertType === 'info' ? 'info' : 'error'"
            variant="flat"
            :loading="firmwareRestarting"
            :disabled="restarting || firmwareRestarting"
            @click="firmwareRestartKlipper"
        >
          {{ t('notifications.klipper.firmware_restart') }}
        </v-btn>
      </div>
    </div>
  </v-alert>
</template>

<style scoped>
:deep(.v-alert__content) {
  display: flex;
  align-items: flex-start;
  padding-top: 2px;
}

:deep(.v-alert__prepend) {
  align-self: flex-start;
  padding-top: 2px;
}

:deep(.v-alert__close) {
  align-self: flex-start;
}

.alert-body {
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-width: 0;
  width: 100%;
}

.alert-body__content {
  min-width: 0;
  width: 100%;
}

.alert-body__title {
  font-weight: 700;
  margin-bottom: 2px;
  line-height: 1.15;
  word-break: break-word;
}

.alert-body__message {
  white-space: pre-wrap;
  word-break: break-word;
  line-height: 1.3;
}

.alert-body__actions {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
  margin-top: 2px;
}
</style>