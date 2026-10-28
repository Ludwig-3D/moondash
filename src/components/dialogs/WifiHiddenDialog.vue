<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import KeyboardOverlay from '../overlays/KeyboardOverlay.vue'

const props = defineProps<{ modelValue: boolean; ssid: string; password: string; connectBusy: boolean }>()
const emit = defineEmits<{
  (e: 'update:modelValue', value: boolean): void
  (e: 'update:ssid', value: string): void
  (e: 'update:password', value: string): void
  (e: 'connect'): void
}>()
const { t } = useI18n()
const open = computed({ get: () => props.modelValue, set: value => emit('update:modelValue', value) })
const ssidValue = computed({ get: () => props.ssid, set: value => emit('update:ssid', value) })
const passwordValue = computed({ get: () => props.password, set: value => emit('update:password', value) })
const activeField = ref<'ssid' | 'password' | null>(null)
const keyboardValue = computed({
  get: () => activeField.value === 'ssid' ? ssidValue.value : passwordValue.value,
  set: value => { if (activeField.value === 'ssid') ssidValue.value = value; else if (activeField.value === 'password') passwordValue.value = value },
})
const keyboardTitle = computed(() => t(activeField.value === 'ssid' ? 'settings.network.wifi.ssid' : 'settings.network.wifi.password'))
watch(() => props.modelValue, value => { if (!value) activeField.value = null })
function keyboardEnter() {
  if (activeField.value === 'ssid') activeField.value = 'password'
  else activeField.value = null
}
</script>

<template>
  <v-dialog v-model="open" max-width="460" @update:model-value="activeField = null">
    <v-card rounded="lg">
      <v-card-title>{{ t('settings.network.wifi.hidden') }}</v-card-title>
      <v-card-text>
        <v-text-field
            v-model="ssidValue"
            :label="t('settings.network.wifi.ssid')"
            variant="outlined"
            density="comfortable"
            hide-details
            readonly
            prepend-inner-icon="mdi-wifi"
            class="mb-4"
            @click="activeField = 'ssid'"
            @focus="activeField = 'ssid'"
        />
        <v-text-field
            v-model="passwordValue"
            :label="t('settings.network.wifi.password')"
            type="password"
            variant="outlined"
            density="comfortable"
            hide-details
            readonly
            prepend-inner-icon="mdi-lock-outline"
            @click="activeField = 'password'"
            @focus="activeField = 'password'"
        />
      </v-card-text>
      <v-card-actions>
        <v-spacer />
        <v-btn @click="open = false">{{ t('settings.network.cancel') }}</v-btn>
        <v-btn color="primary" variant="tonal" :loading="connectBusy" @click="emit('connect')">
          {{ t('settings.network.connect') }}
        </v-btn>
      </v-card-actions>
    </v-card>
  </v-dialog>
  <KeyboardOverlay
      v-model="keyboardValue"
      :visible="open && activeField !== null"
      :title="keyboardTitle"
      @enter="keyboardEnter"
      @close="activeField = null"
  />
</template>
