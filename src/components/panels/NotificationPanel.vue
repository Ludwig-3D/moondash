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

const SIMULATE_AFC_ALERT = false
const SIMULATE_KLIPPER_ALERT = false
const SIMULATE_MOONRAKER_ALERT = false

function extractAfcMessage(): string {
  const objects = moonraker.value.afc.objects as Record<string, any>
  const afcRoot = objects.AFC ?? {}

  const candidates: unknown[] = [
    afcRoot.message,
    afcRoot.error,
    afcRoot.alert,
    afcRoot.current_message,
    afcRoot.last_message,
  ]

  for (const value of candidates) {
    if (typeof value === 'string' && value.trim()) return value.trim()
  }

  for (const [, value] of Object.entries(objects)) {
    if (!value || typeof value !== 'object' || Array.isArray(value)) continue

    const record = value as Record<string, unknown>
    const nestedCandidates = [
      record.message,
      record.error,
      record.alert,
      record.current_message,
      record.last_message,
    ]

    for (const candidate of nestedCandidates) {
      if (typeof candidate === 'string' && candidate.trim()) {
        return candidate.trim()
      }
    }
  }

  return ''
}

function isBenignKlipperMessage(message: string): boolean {
  const normalized = message.trim().toLowerCase()

  return [
    '',
    'printer is ready',
    'ready',
  ].includes(normalized)
}

const afcMessage = computed(() => {
  if (SIMULATE_AFC_ALERT) {
    return 'AFC lane 4 is empty. Please reload filament or assign a different lane.'
  }

  return extractAfcMessage()
})

const klipperState = computed(() => moonraker.value.webhooks.state?.toLowerCase() ?? '')
const klipperStateMessage = computed(() => moonraker.value.webhooks.stateMessage?.trim() ?? '')
const klipperPrintMessage = computed(() => moonraker.value.printStats.message?.trim() ?? '')

const showMoonrakerAlert = computed(() => {
  if (SIMULATE_MOONRAKER_ALERT) return true
  return !websocket.value.connected
})

const moonrakerMessage = computed(() => {
  if (SIMULATE_MOONRAKER_ALERT) {
    return 'Moonraker is disconnected or Klippy is not reachable.'
  }

  return t('notifications.moonraker.message_disconnected')
})

const showKlipperAlert = computed(() => {
  if (SIMULATE_KLIPPER_ALERT) return true
  if (!websocket.value.connected) return false

  const state = klipperState.value
  const stateMessage = klipperStateMessage.value
  const printMessage = klipperPrintMessage.value

  return Boolean(
      state === 'startup' ||
      state === 'shutdown' ||
      state === 'error' ||
      !moonrakerReady.value ||
      !isBenignKlipperMessage(stateMessage) ||
      !isBenignKlipperMessage(printMessage),
  )
})

const klipperTitleKey = computed(() => {
  if (SIMULATE_KLIPPER_ALERT) return 'notifications.klipper.title_error'

  if (klipperState.value === 'shutdown') {
    return 'notifications.klipper.title_shutdown'
  }

  if (klipperState.value === 'error') {
    return 'notifications.klipper.title_error'
  }

  if (
      klipperState.value === 'startup' ||
      (websocket.value.connected && !moonrakerReady.value)
  ) {
    return 'notifications.klipper.title_startup'
  }

  return 'notifications.klipper.title'
})

const klipperMessage = computed(() => {
  if (SIMULATE_KLIPPER_ALERT) {
    return 'MCU shutdown: Heater extruder not heating'
  }

  if (!isBenignKlipperMessage(klipperStateMessage.value)) {
    return klipperStateMessage.value
  }

  if (!isBenignKlipperMessage(klipperPrintMessage.value)) {
    return klipperPrintMessage.value
  }

  if (
      klipperState.value === 'startup' ||
      (websocket.value.connected && !moonrakerReady.value)
  ) {
    return t('notifications.klipper.message_startup')
  }

  return ''
})

const hasAlerts = computed(() => {
  return showMoonrakerAlert.value || showKlipperAlert.value || Boolean(afcMessage.value)
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