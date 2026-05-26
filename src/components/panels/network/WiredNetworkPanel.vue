<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

type WiredInterface = {
  interfaceName: string
  connected: boolean
  ip: string | null
}

type WiredSettings = {
  interfaces: WiredInterface[]
}

const wiredSettings = ref<WiredSettings | null>(null)
const wiredBusy = ref<string | null>(null)

const wiredInterfaces = computed(() => wiredSettings.value?.interfaces ?? [])

async function loadWiredSettings() {
  wiredSettings.value = await invoke<WiredSettings>('get_wired_settings')
}

async function toggleWired(interfaceName: string, enabled: boolean) {
  try {
    wiredBusy.value = interfaceName

    await invoke('set_wired_interface_enabled', {
      interfaceName,
      enabled,
    })

    await loadWiredSettings()
  } finally {
    wiredBusy.value = null
  }
}

onMounted(loadWiredSettings)
</script>

<template>
  <v-card rounded="lg" variant="flat">
    <v-card-title>
      {{ t('settings.network.wired.title') }}
    </v-card-title>

    <v-card-text class="network-panel__card-content">
      <v-list density="compact" bg-color="transparent">
        <v-list-item
            v-for="iface in wiredInterfaces"
            :key="iface.interfaceName"
            prependGap="1em"
            class="px-0"
        >
          <template #prepend>
            <v-icon :color="iface.connected ? 'success' : undefined" style="font-size: 2.25em">
              mdi-ethernet
            </v-icon>
          </template>

          <v-list-item-title>{{ iface.interfaceName }}</v-list-item-title>
          <v-list-item-subtitle>{{ iface.ip || '--' }}</v-list-item-subtitle>

          <template #append>
            <v-switch
                :model-value="iface.connected"
                color="primary"
                hide-details
                density="compact"
                inset
                :disabled="wiredBusy === iface.interfaceName"
                @update:model-value="(value) => toggleWired(iface.interfaceName, Boolean(value))"
            />
          </template>
        </v-list-item>

        <v-list-item v-if="!wiredInterfaces.length">
          <v-list-item-title>
            {{ t('settings.network.wired.no_interfaces') }}
          </v-list-item-title>
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
</style>