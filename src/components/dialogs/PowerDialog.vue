<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { moonraker as moonrakerClient } from '@/plugins/moonraker'

const { t } = useI18n()

const props = defineProps<{
  modelValue: boolean
}>()

const emit = defineEmits<{
  (e: 'update:modelValue', value: boolean): void
}>()

const loadingAction = ref<string | null>(null)

const dialogOpen = computed({
  get: () => props.modelValue,
  set: (value: boolean) => emit('update:modelValue', value),
})

async function runAction(
    action: 'shutdown' | 'reboot' | 'restart_klipper' | 'restart_firmware',
) {
  if (loadingAction.value) return

  try {
    loadingAction.value = action

    if (action === 'shutdown') {
      await moonrakerClient.call('machine.shutdown')
    } else if (action === 'reboot') {
      await moonrakerClient.call('machine.reboot')
    } else if (action === 'restart_klipper') {
      await moonrakerClient.call('printer.restart')
    } else if (action === 'restart_firmware') {
      await moonrakerClient.call('printer.firmware_restart')
    }

    dialogOpen.value = false
  } finally {
    loadingAction.value = null
  }
}
</script>

<template>
  <v-dialog
      v-model="dialogOpen"
      max-width="760"
      persistent
  >
    <v-card rounded="lg">
      <v-card-title>
        {{ t('power_dialog.title') }}
      </v-card-title>

      <v-card-text class="power-dialog__content">
        <div class="power-dialog__grid">
          <div class="power-dialog__tile">
            <v-btn
                class="power-dialog__btn"
                color="error"
                variant="tonal"
                :loading="loadingAction === 'shutdown'"
                :disabled="Boolean(loadingAction)"
                @click="runAction('shutdown')"
            >
              <div class="power-dialog__btn-content">
                <v-icon size="18">mdi-power</v-icon>
                <span>{{ t('power_dialog.shutdown') }}</span>
              </div>
            </v-btn>
          </div>

          <div class="power-dialog__tile">
            <v-btn
                class="power-dialog__btn"
                color="warning"
                variant="tonal"
                :loading="loadingAction === 'reboot'"
                :disabled="Boolean(loadingAction)"
                @click="runAction('reboot')"
            >
              <div class="power-dialog__btn-content">
                <v-icon size="18">mdi-restart</v-icon>
                <span>{{ t('power_dialog.reboot') }}</span>
              </div>
            </v-btn>
          </div>

          <div class="power-dialog__tile">
            <v-btn
                class="power-dialog__btn"
                variant="tonal"
                :loading="loadingAction === 'restart_klipper'"
                :disabled="Boolean(loadingAction)"
                @click="runAction('restart_klipper')"
            >
              <div class="power-dialog__btn-content">
                <v-icon size="18">mdi-refresh</v-icon>
                <span>{{ t('power_dialog.restart_klipper') }}</span>
              </div>
            </v-btn>
          </div>

          <div class="power-dialog__tile">
            <v-btn
                class="power-dialog__btn"
                color="info"
                variant="tonal"
                :loading="loadingAction === 'restart_firmware'"
                :disabled="Boolean(loadingAction)"
                @click="runAction('restart_firmware')"
            >
              <div class="power-dialog__btn-content">
                <v-icon size="32">mdi-chip</v-icon>
                <span>{{ t('power_dialog.restart_firmware') }}</span>
              </div>
            </v-btn>
          </div>
        </div>
      </v-card-text>

      <v-card-actions>
        <v-spacer />
        <v-btn
            variant="text"
            :disabled="Boolean(loadingAction)"
            @click="dialogOpen = false"
        >
          {{ t('power_dialog.close') }}
        </v-btn>
      </v-card-actions>
    </v-card>
  </v-dialog>
</template>

<style scoped>
.power-dialog__content {
  padding-top: 12px;
}

.power-dialog__grid {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 14px;
}

.power-dialog__tile {
  position: relative;
  width: 100%;
  aspect-ratio: 1 / 1;
}

.power-dialog__btn {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  min-width: 0;
  padding: 0;
}

.power-dialog__btn-content {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  width: 100%;
  height: 100%;
  text-align: center;
}

.power-dialog__btn-content span {
  font-size: 0.75rem; /* smaller */
  line-height: 1.1;
  max-width: 100%;
  letter-spacing: 0.04em; /* gives nicer UI feel */
}

.power-dialog__btn :deep(.v-btn__content) {
  width: 100%;
  height: 100%;
}
</style>