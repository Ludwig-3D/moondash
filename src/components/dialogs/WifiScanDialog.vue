<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { WifiNetwork } from '@/stores/app'
import LoadingActionIcon from '@/components/common/LoadingActionIcon.vue'

const { t } = useI18n()
const props = defineProps<{
  modelValue: boolean
  networks: WifiNetwork[]
  connectBusy: boolean
  connectingSsid: string | null
}>()
const emit = defineEmits<{
  (e: 'update:modelValue', value: boolean): void
  (e: 'select', network: WifiNetwork): void
}>()
const open = computed({
  get: () => props.modelValue,
  set: value => emit('update:modelValue', value),
})
function signalIcon(signal: number | null): string {
  if (signal === null) return 'mdi-wifi-strength-outline'
  if (signal >= 80) return 'mdi-wifi-strength-4'
  if (signal >= 60) return 'mdi-wifi-strength-3'
  if (signal >= 40) return 'mdi-wifi-strength-2'
  if (signal >= 20) return 'mdi-wifi-strength-1'
  return 'mdi-wifi-strength-outline'
}
</script>

<template>
  <v-dialog v-model="open" max-width="720">
    <v-card class="wifi-dialog__card" rounded="lg">
      <v-card-title>{{ t('settings.network.wifi.available') }}</v-card-title>
      <v-card-text class="wifi-dialog__content">
        <v-list v-if="networks.length" class="wifi-dialog__list" density="compact" bg-color="transparent">
          <v-list-item
              v-for="network in networks"
              :key="network.ssid"
              :disabled="connectBusy"
              @click="!connectBusy && emit('select', network)"
          >
            <template #prepend>
              <v-icon :icon="signalIcon(network.signalPercent)" />
            </template>
            <v-list-item-title>{{ network.ssid }}</v-list-item-title>
            <v-list-item-subtitle>
              {{ network.signalPercent ?? 0 }}%
              <span v-if="network.saved" class="text-success font-weight-medium"> · {{ t('settings.network.wifi.saved') }}</span>
            </v-list-item-subtitle>
            <template #append>
              <LoadingActionIcon
                v-if="connectBusy && connectingSsid === network.ssid"
                icon="mdi-wifi"
                :loading="true"
              />
              <v-icon v-else-if="network.secured" icon="mdi-lock" size="small" />
            </template>
          </v-list-item>
        </v-list>
        <v-alert v-else type="info" variant="tonal">
          {{ t('settings.network.wifi.no_networks') }}
        </v-alert>
      </v-card-text>
      <v-card-actions>
        <v-spacer />
        <v-btn @click="open = false">{{ t('settings.network.close') }}</v-btn>
      </v-card-actions>
    </v-card>
  </v-dialog>
</template>