<script setup lang="ts">
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { moonraker as moonrakerClient } from '@/plugins/moonraker'

const { t } = useI18n()

defineProps<{
  message: string
}>()

const reconnecting = ref(false)

async function reconnectMoonraker() {
  if (reconnecting.value) return

  try {
    reconnecting.value = true
    await moonrakerClient.reconnect()
  } catch {
    // ignore
  } finally {
    reconnecting.value = false
  }
}
</script>

<template>
  <v-alert
      type="info"
      variant="tonal"
      border="start"
      density="compact"
  >
    <div class="alert-body">
      <div class="alert-body__content">
        <div class="alert-body__title">{{ t('notifications.moonraker.title') }}</div>
        <div class="alert-body__message">
          {{ message }}
        </div>
      </div>

      <div class="alert-body__actions">
        <v-btn
            color="info"
            variant="flat"
            :loading="reconnecting"
            :disabled="reconnecting"
            @click="reconnectMoonraker"
        >
          {{ t('notifications.moonraker.reconnect') }}
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