<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useAppStore, type WifiNetwork } from '@/stores/app'
import WifiPasswordDialog from '../../dialogs/WifiPasswordDialog.vue'
import WifiHiddenDialog from '../../dialogs/WifiHiddenDialog.vue'
import WifiScanDialog from '../../dialogs/WifiScanDialog.vue'
import WifiSavedDialog from '../../dialogs/WifiSavedDialog.vue'

const { t } = useI18n()
const appStore = useAppStore()

const wifiBusy = ref(false)
const scanBusy = ref(false)
const connectBusy = ref(false)
const forgetBusy = ref<string | null>(null)

const scanDialogOpen = ref(false)
const savedDialogOpen = ref(false)
const hiddenDialogOpen = ref(false)
const passwordDialogOpen = ref(false)

const selectedNetwork = ref<WifiNetwork | null>(null)
const wifiPassword = ref('')
const hiddenSsid = ref('')
const hiddenPassword = ref('')
const wifiSettings = computed(() => appStore.getWifiSettings)
const wifiEnabled = computed(() => wifiSettings.value?.enabled ?? false)
const connectedSsid = computed(() => wifiSettings.value?.connectedSsid ?? null)
const connectedIp = computed(() => wifiSettings.value?.connectedIp ?? null)
const scannedNetworks = computed(() => wifiSettings.value?.scannedNetworks ?? [])
const savedNetworks = computed(() => wifiSettings.value?.savedNetworks ?? [])

async function toggleWifi(value: boolean | null) {
  wifiBusy.value = true

  try {
    await appStore.setWifiEnabled(Boolean(value))
  } finally {
    wifiBusy.value = false
  }
}

async function scanWifiNetworks() {
  scanBusy.value = true

  try {
    await appStore.scanWifiNetworks()
    scanDialogOpen.value = true
  } finally {
    scanBusy.value = false
  }
}

function openPasswordDialog(network: WifiNetwork) {
  selectedNetwork.value = network
  wifiPassword.value = ''
  passwordDialogOpen.value = true
}

async function connectNetwork(network: WifiNetwork, password?: string) {
  connectBusy.value = true

  try {
    await appStore.connectToWifi(network.ssid, password || null)

    scanDialogOpen.value = false
    savedDialogOpen.value = false
    passwordDialogOpen.value = false
  } finally {
    connectBusy.value = false
  }
}

async function connectSelectedNetwork() {
  if (!selectedNetwork.value) return
  await connectNetwork(selectedNetwork.value, wifiPassword.value)
}

async function connectHiddenNetwork() {
  connectBusy.value = true

  try {
    await appStore.connectHiddenWifi(hiddenSsid.value, hiddenPassword.value || null)

    hiddenDialogOpen.value = false
    hiddenSsid.value = ''
    hiddenPassword.value = ''
  } finally {
    connectBusy.value = false
  }
}

async function forgetNetwork(network: WifiNetwork) {
  forgetBusy.value = network.ssid

  try {
    await appStore.forgetSavedWifi(network.ssid)
  } finally {
    forgetBusy.value = null
  }
}

function selectNetwork(network: WifiNetwork) {
  if (network.secured && !network.saved) {
    openPasswordDialog(network)
    return
  }

  void connectNetwork(network)
}
</script>

<template>
  <v-card rounded="lg" variant="flat">
    <v-card-title class="network-panel__title-row">
      <span>{{ t('settings.network.wifi.title') }}</span>

      <div class="network-panel__wifi-controls">
        <v-btn
            icon="mdi-magnify"
            variant="text"
            :loading="scanBusy"
            :disabled="!wifiEnabled"
            @click="scanWifiNetworks"
        />

        <v-btn
            icon="mdi-key-plus"
            variant="text"
            @click="hiddenDialogOpen = true"
        />

        <v-switch
            :model-value="wifiEnabled"
            color="primary"
            hide-details
            density="compact"
            inset
            :loading="wifiBusy"
            :disabled="wifiBusy"
            @update:model-value="toggleWifi"
        />
      </div>
    </v-card-title>

    <v-card-text class="network-panel__card-content">
      <div class="network-panel__info-row">
        <span class="network-panel__label">
          {{ t('settings.network.wifi.connected_ssid') }}
        </span>

        <span class="network-panel__value">
          {{ connectedSsid || t('settings.network.none') }}
        </span>
      </div>

      <div class="network-panel__info-row">
        <span class="network-panel__label">
          {{ t('settings.network.wifi.ip') }}
        </span>

        <span class="network-panel__value">
          {{ connectedIp || t('settings.network.none') }}
        </span>
      </div>

      <v-btn
          variant="tonal"
          prepend-icon="mdi-content-save"
          @click="savedDialogOpen = true"
      >
        {{ t('settings.network.wifi.saved_networks') }}
      </v-btn>
    </v-card-text>
  </v-card>

  <WifiScanDialog
      v-model="scanDialogOpen"
      :networks="scannedNetworks"
      @select="selectNetwork"
  />

  <WifiSavedDialog
      v-model="savedDialogOpen"
      :networks="savedNetworks"
      :connect-busy="connectBusy"
      :forget-busy="forgetBusy"
      @connect="connectNetwork"
      @forget="forgetNetwork"
  />

  <WifiPasswordDialog
      v-model="passwordDialogOpen"
      v-model:password="wifiPassword"
      :ssid="selectedNetwork?.ssid ?? ''"
      :connect-busy="connectBusy"
      @connect="connectSelectedNetwork"
  />

  <WifiHiddenDialog
      v-model="hiddenDialogOpen"
      v-model:ssid="hiddenSsid"
      v-model:password="hiddenPassword"
      :connect-busy="connectBusy"
      @connect="connectHiddenNetwork"
  />
</template>

<style scoped>
.network-panel__title-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
}

.network-panel__wifi-controls {
  display: flex;
  align-items: center;
  gap: 4px;
}

.network-panel__card-content {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.network-panel__info-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
}

.network-panel__label {
  opacity: 0.7;
}

.network-panel__value {
  font-weight: 600;
  text-align: right;
}

</style>
