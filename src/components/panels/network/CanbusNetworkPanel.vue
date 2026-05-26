<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

type CanbusInterface = {
  interfaceName: string
  connected: boolean
  bitrate?: number | null
  rxBytes?: number | null
  txBytes?: number | null
  rxPackets?: number | null
  txPackets?: number | null
  rxBytesPerSecond?: number | null
  txBytesPerSecond?: number | null
  bandwidth?: number | null
}

type CanbusSettings = {
  interfaces: CanbusInterface[]
}

const canbusSettings = ref<CanbusSettings | null>(null)
let canbusRefreshTimer: number | null = null

const canbusInterfaces = computed(() => canbusSettings.value?.interfaces ?? [])

async function loadCanbusSettings() {
  try {
    canbusSettings.value = await invoke<CanbusSettings>('get_canbus_settings')
  } catch {
    canbusSettings.value = null
  }
}

function formatBytes(value: number | null | undefined, fractionDigits = 1): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return '--'

  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  let size = Math.max(0, value)
  let unitIndex = 0

  while (size >= 1024 && unitIndex < units.length - 1) {
    size /= 1024
    unitIndex += 1
  }

  return `${size.toFixed(unitIndex === 0 ? 0 : fractionDigits)} ${units[unitIndex]}`
}

function formatBytesPerSecond(value: number | null | undefined): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return '--'
  return `${formatBytes(value)}/s`
}

function formatBitrate(value: number | null | undefined): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return '--'

  const units = ['bit/s', 'kbit/s', 'Mbit/s', 'Gbit/s']
  let size = Math.max(0, value)
  let unitIndex = 0

  while (size >= 1000 && unitIndex < units.length - 1) {
    size /= 1000
    unitIndex += 1
  }

  const precision = unitIndex === 0 ? 0 : 1
  return `${size.toFixed(precision)} ${units[unitIndex]}`
}

function formatCanbusSubtitle(iface: CanbusInterface): string {
  const parts: string[] = []

  if (iface.bitrate) {
    parts.push(formatBitrate(iface.bitrate))
  }

  const rxRate = iface.rxBytesPerSecond ?? null
  const txRate = iface.txBytesPerSecond ?? null

  if (rxRate !== null || txRate !== null) {
    parts.push(`RX ${formatBytesPerSecond(rxRate)} / TX ${formatBytesPerSecond(txRate)}`)
  } else if (iface.bandwidth !== null && iface.bandwidth !== undefined) {
    parts.push(`${formatBytesPerSecond(iface.bandwidth)} total`)
  }

  return parts.length ? parts.join(' · ') : '--'
}

onMounted(async () => {
  await loadCanbusSettings()

  canbusRefreshTimer = window.setInterval(() => {
    void loadCanbusSettings()
  }, 1000)
})

onBeforeUnmount(() => {
  if (canbusRefreshTimer !== null) {
    window.clearInterval(canbusRefreshTimer)
    canbusRefreshTimer = null
  }
})
</script>

<template>
  <v-card class="network-panel__canbus-card" rounded="lg" variant="flat" v-if="canbusInterfaces.length">
    <v-card-title>
      {{ t('settings.network.canbus.title') }}
    </v-card-title>

    <v-card-text class="network-panel__card-content">
      <v-list density="compact" bg-color="transparent">
        <v-list-item
            v-for="iface in canbusInterfaces"
            :key="iface.interfaceName"
            prependGap="1em"
            class="px-0"
        >
          <template #prepend>
            <v-icon :color="iface.connected ? 'success' : undefined" style="font-size: 2.25em">
              mdi-expansion-card
            </v-icon>
          </template>

          <v-list-item-title>{{ iface.interfaceName }}</v-list-item-title>

          <v-list-item-subtitle>
            {{ formatCanbusSubtitle(iface) }}
          </v-list-item-subtitle>
        </v-list-item>
      </v-list>
    </v-card-text>
  </v-card>
</template>

<style scoped>
.network-panel__card-content {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.network-panel__canbus-card {
  grid-column: 2;
}
</style>