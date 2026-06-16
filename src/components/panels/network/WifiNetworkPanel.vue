<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useAppStore, type WifiNetwork } from '@/stores/app'

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

function signalIcon(signal: number | null): string {
  if (signal === null) return 'mdi-wifi-strength-outline'
  if (signal >= 80) return 'mdi-wifi-strength-4'
  if (signal >= 60) return 'mdi-wifi-strength-3'
  if (signal >= 40) return 'mdi-wifi-strength-2'
  if (signal >= 20) return 'mdi-wifi-strength-1'
  return 'mdi-wifi-strength-outline'
}

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

  <v-dialog v-model="scanDialogOpen" max-width="720">
    <v-card rounded="lg">
      <v-card-title>{{ t('settings.network.wifi.available') }}</v-card-title>

      <v-card-text>
        <v-list v-if="scannedNetworks.length" density="compact">
          <v-list-item
              v-for="network in scannedNetworks"
              :key="network.ssid"
              @click="selectNetwork(network)"
          >
            <template #prepend>
              <v-icon :icon="signalIcon(network.signalPercent)" />
            </template>

            <v-list-item-title>{{ network.ssid }}</v-list-item-title>

            <v-list-item-subtitle>
              {{ network.signalPercent ?? 0 }}%
              <span v-if="network.saved"> · {{ t('settings.network.wifi.saved') }}</span>
            </v-list-item-subtitle>

            <template #append>
              <v-icon v-if="network.secured" icon="mdi-lock" size="small" />
            </template>
          </v-list-item>
        </v-list>

        <v-alert v-else type="info" variant="tonal">
          {{ t('settings.network.wifi.no_networks') }}
        </v-alert>
      </v-card-text>

      <v-card-actions>
        <v-spacer />
        <v-btn @click="scanDialogOpen = false">
          {{ t('settings.network.close') }}
        </v-btn>
      </v-card-actions>
    </v-card>
  </v-dialog>

  <v-dialog v-model="savedDialogOpen" max-width="720">
    <v-card rounded="lg">
      <v-card-title>{{ t('settings.network.wifi.saved_networks') }}</v-card-title>

      <v-card-text>
        <v-list v-if="savedNetworks.length" density="compact">
          <v-list-item
              v-for="network in savedNetworks"
              :key="network.ssid"
          >
            <template #prepend>
              <v-icon :icon="signalIcon(network.signalPercent)" />
            </template>

            <v-list-item-title>{{ network.ssid }}</v-list-item-title>

            <v-list-item-subtitle>
              {{ network.signalPercent !== null ? `${network.signalPercent}%` : t('settings.network.none') }}
            </v-list-item-subtitle>

            <template #append>
              <div class="network-panel__saved-actions">
                <v-btn
                    icon="mdi-wifi"
                    variant="text"
                    size="small"
                    :loading="connectBusy"
                    :aria-label="t('settings.network.connect')"
                    @click="connectNetwork(network)"
                />

                <v-btn
                    icon="mdi-delete"
                    variant="text"
                    size="small"
                    color="error"
                    :loading="forgetBusy === network.ssid"
                    @click="forgetNetwork(network)"
                />
              </div>
            </template>
          </v-list-item>
        </v-list>

        <v-alert v-else type="info" variant="tonal">
          {{ t('settings.network.wifi.no_saved_networks') }}
        </v-alert>
      </v-card-text>

      <v-card-actions>
        <v-spacer />
        <v-btn @click="savedDialogOpen = false">
          {{ t('settings.network.close') }}
        </v-btn>
      </v-card-actions>
    </v-card>
  </v-dialog>

  <v-dialog v-model="passwordDialogOpen" max-width="460">
    <v-card rounded="lg">
      <v-card-title>
        {{ t('settings.network.wifi.connect_to', { ssid: selectedNetwork?.ssid }) }}
      </v-card-title>

      <v-card-text>
        <v-text-field
            v-model="wifiPassword"
            :label="t('settings.network.wifi.password')"
            type="password"
            autofocus
            @keyup.enter="connectSelectedNetwork"
        />
      </v-card-text>

      <v-card-actions>
        <v-spacer />

        <v-btn @click="passwordDialogOpen = false">
          {{ t('settings.network.cancel') }}
        </v-btn>

        <v-btn
            color="primary"
            variant="tonal"
            :loading="connectBusy"
            @click="connectSelectedNetwork"
        >
          {{ t('settings.network.connect') }}
        </v-btn>
      </v-card-actions>
    </v-card>
  </v-dialog>

  <v-dialog v-model="hiddenDialogOpen" max-width="460">
    <v-card rounded="lg">
      <v-card-title>{{ t('settings.network.wifi.hidden') }}</v-card-title>

      <v-card-text>
        <v-text-field
            v-model="hiddenSsid"
            :label="t('settings.network.wifi.ssid')"
            autofocus
        />

        <v-text-field
            v-model="hiddenPassword"
            :label="t('settings.network.wifi.password')"
            type="password"
            @keyup.enter="connectHiddenNetwork"
        />
      </v-card-text>

      <v-card-actions>
        <v-spacer />

        <v-btn @click="hiddenDialogOpen = false">
          {{ t('settings.network.cancel') }}
        </v-btn>

        <v-btn
            color="primary"
            variant="tonal"
            :loading="connectBusy"
            @click="connectHiddenNetwork"
        >
          {{ t('settings.network.connect') }}
        </v-btn>
      </v-card-actions>
    </v-card>
  </v-dialog>
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

.network-panel__saved-actions {
  display: flex;
  align-items: center;
  gap: 4px;
}
</style>