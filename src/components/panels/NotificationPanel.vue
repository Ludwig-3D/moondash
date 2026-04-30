<script setup lang="ts">
import { computed } from 'vue'
import { storeToRefs } from 'pinia'
import { useI18n } from 'vue-i18n'
import { useAppStore } from '@/stores/app'
import AfcAlert from '@/components/notifications/AfcAlert.vue'
import KlipperAlert from '@/components/notifications/KlipperAlert.vue'
import MoonrakerAlert from '@/components/notifications/MoonrakerAlert.vue'

const { t } = useI18n()

const appStore = useAppStore()
const { moonraker, websocket, moonrakerReady } = storeToRefs(appStore)

function isBenignKlipperMessage(message: string): boolean {
  const normalized = message.trim().toLowerCase()

  return [
    '',
    'printer is ready',
    'ready',
  ].includes(normalized)
}

function findMessageDeep(value: unknown): string {
  if (!value || typeof value !== 'object') return ''

  const record = value as Record<string, unknown>

  for (const key of [
    'message',
    'error',
    'alert',
    'current_message',
    'last_message',
    'state_message',
    'stateMessage',
  ]) {
    const candidate = record[key]
    if (typeof candidate === 'string' && candidate.trim()) {
      return candidate.trim()
    }
  }

  for (const child of Object.values(record)) {
    if (child && typeof child === 'object' && !Array.isArray(child)) {
      const found = findMessageDeep(child)
      if (found) return found
    }
  }

  return ''
}

const afcMessage = computed(() => {
  return findMessageDeep(moonraker.value.afc)
})

const klipperState = computed(() => {
  return moonraker.value.webhooks.state?.trim().toLowerCase() ?? ''
})

const klipperStateMessage = computed(() => {
  return moonraker.value.webhooks.stateMessage?.trim() ?? ''
})

const klipperPrintMessage = computed(() => {
  return moonraker.value.printStats.message?.trim() ?? ''
})

const showMoonrakerAlert = computed(() => {
  return !websocket.value.connected
})

const moonrakerMessage = computed(() => {
  return t('notifications.moonraker.message_disconnected')
})

const showKlipperAlert = computed(() => {
  if (!websocket.value.connected) return false

  const state = klipperState.value

  if (state === 'startup') return true
  if (state === 'shutdown') return true
  if (state === 'error') return true
  if (moonrakerReady.value === false) return true

  if (!isBenignKlipperMessage(klipperStateMessage.value)) return true
  if (!isBenignKlipperMessage(klipperPrintMessage.value)) return true

  return false
})

const klipperTitleKey = computed(() => {
  if (klipperState.value === 'shutdown') {
    return 'notifications.klipper.title_shutdown'
  }

  if (klipperState.value === 'error') {
    return 'notifications.klipper.title_error'
  }

  if (
      klipperState.value === 'startup' ||
      moonrakerReady.value === false
  ) {
    return 'notifications.klipper.title_startup'
  }

  return 'notifications.klipper.title'
})

const klipperMessage = computed(() => {
  if (!isBenignKlipperMessage(klipperStateMessage.value)) {
    return klipperStateMessage.value
  }

  if (!isBenignKlipperMessage(klipperPrintMessage.value)) {
    return klipperPrintMessage.value
  }

  if (
      klipperState.value === 'startup' ||
      moonrakerReady.value === false
  ) {
    return t('notifications.klipper.message_startup')
  }

  return ''
})

const hasAlerts = computed(() => {
  return showMoonrakerAlert.value ||
      showKlipperAlert.value ||
      Boolean(afcMessage.value)
})
</script>

<template>
  <div
      v-if="hasAlerts"
      class="pt-2 pr-2 pb-2 notifications-container"
  >
    <v-card
        class="notifications-panel"
        rounded="lg"
        variant="flat"
    >
      <v-card-text class="notifications-panel__content pa-0">
        <MoonrakerAlert
            v-if="showMoonrakerAlert"
            :message="moonrakerMessage"
        />

        <KlipperAlert
            v-if="showKlipperAlert"
            :title-key="klipperTitleKey"
            :message="klipperMessage"
        />

        <AfcAlert
            v-if="afcMessage"
            :message="afcMessage"
        />
      </v-card-text>
    </v-card>
  </div>
</template>

<style scoped>
.notifications-container {
  max-height: 50%;
  min-height: 0;
}

.notifications-panel {
  width: 40vw;
  max-width: 40vw !important;
  max-height: 100%;
  min-height: 0;
  overflow: hidden;
}

.notifications-panel__content {
  display: flex;
  flex-direction: column;
  align-items: stretch;
  gap: 12px;
  max-height: calc(100vh - 16px);
  height: calc(100vh - 16px);
  min-height: 0;
  overflow-y: auto;
  overflow-x: hidden;
}

.notifications-panel__content > * {
  flex: 0 0 auto;
}
</style>