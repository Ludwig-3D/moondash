<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import KeyboardOverlay from '../overlays/KeyboardOverlay.vue'

const props = defineProps<{ modelValue: boolean; ssid: string; password: string; connectBusy: boolean }>()
const emit = defineEmits<{
  (e: 'update:modelValue', value: boolean): void
  (e: 'update:password', value: string): void
  (e: 'connect'): void
}>()
const { t } = useI18n()
const open = computed({ get: () => props.modelValue, set: value => emit('update:modelValue', value) })
const passwordValue = computed({ get: () => props.password, set: value => emit('update:password', value) })
const keyboardVisible = ref(false)
watch(() => props.modelValue, value => { if (!value) keyboardVisible.value = false })
</script>

<template>
  <v-dialog v-model="open" max-width="460" @update:model-value="keyboardVisible = false">
    <v-card rounded="lg">
      <v-card-title>{{ t('settings.network.wifi.connect_to', { ssid }) }}</v-card-title>
      <v-card-text>
        <v-text-field
            v-model="passwordValue"
            :label="t('settings.network.wifi.password')"
            type="password"
            variant="outlined"
            density="comfortable"
            hide-details
            readonly
            prepend-inner-icon="mdi-lock-outline"
            @click="keyboardVisible = true"
            @focus="keyboardVisible = true"
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
      v-model="passwordValue"
      :visible="open && keyboardVisible"
      :title="t('settings.network.wifi.password')"
      @enter="keyboardVisible = false"
      @close="keyboardVisible = false"
  />
</template>
