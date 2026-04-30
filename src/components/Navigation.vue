<script setup lang="ts">
import { computed, ref } from 'vue'
import { storeToRefs } from 'pinia'
import { useAppStore } from '@/stores/app'
import { moonraker as moonrakerClient } from '@/plugins/moonraker'
import PowerDialog from '@/components/dialogs/PowerDialog.vue'

const appStore = useAppStore()
const { isDebugEnabled, moonraker } = storeToRefs(appStore)

const powerDialogOpen = ref(false)
const emergencyStopping = ref(false)

const printerState = computed(() => moonraker.value.printStats.state?.toLowerCase() ?? '')

const isPrinterRunning = computed(() => {
  return ['printing', 'paused', 'pausing', 'resuming'].includes(printerState.value)
})

async function emergencyStop() {
  if (emergencyStopping.value) return

  try {
    emergencyStopping.value = true
    await moonrakerClient.call('printer.emergency_stop')
  } finally {
    emergencyStopping.value = false
  }
}

function onPowerClick() {
  if (isPrinterRunning.value) {
    void emergencyStop()
  } else {
    powerDialogOpen.value = true
  }
}
</script>

<template>
  <v-navigation-drawer rail permanent class="sidenav" width="58" rail-width="58">
    <div class="nav-list">
      <RouterLink class="nav-button" active-class="nav-button--active" to="/">
        <v-icon icon="mdi-home-variant" />
      </RouterLink>

      <RouterLink class="nav-button" active-class="nav-button--active" to="/tune">
        <v-icon icon="mdi-tune-vertical-variant" />
      </RouterLink>

      <RouterLink class="nav-button" active-class="nav-button--active" to="/files">
        <v-icon icon="mdi-printer-3d" />
      </RouterLink>

      <RouterLink class="nav-button" active-class="nav-button--active" to="/settings">
        <v-icon icon="mdi-cog" />
      </RouterLink>

      <RouterLink
          v-if="isDebugEnabled"
          class="nav-button"
          active-class="nav-button--active"
          to="/dev"
      >
        <v-icon icon="mdi-code-json" />
      </RouterLink>

      <button
          class="nav-button"
          type="button"
          :disabled="emergencyStopping"
          @click="onPowerClick"
      >
        <v-progress-circular
            v-if="emergencyStopping"
            indeterminate
            size="16"
            width="2"
            color="error"
        />

        <v-icon v-else :color="isPrinterRunning ? 'error' : undefined">
          {{ isPrinterRunning ? 'mdi-alert-octagon' : 'mdi-power' }}
        </v-icon>
      </button>
    </div>
  </v-navigation-drawer>

  <PowerDialog v-model="powerDialogOpen" />
</template>

<style scoped>
.nav-list {
  height: 100vh;
  width: 58px;
  display: flex;
  flex-direction: column;
  padding: 0;
  margin: 0;
  gap: 5px;
}

.nav-button {
  flex: 1;
  width: 58px;
  min-height: 0;
  padding: 0;
  border: 0;
  border-radius: 0;
  background: transparent;
  color: rgba(var(--v-theme-on-background), 0.8);
  display: flex;
  align-items: center;
  justify-content: center;
  text-decoration: none;
  cursor: pointer;
  font-size: 1.45em;
}

.nav-button--active {
  color: rgb(var(--v-theme-primary));
}

.nav-button:disabled {
  opacity: 0.5;
  cursor: default;
}

.sidenav {
  width: 58px !important;
  background: rgb(var(--v-theme-background));
  border: none;
}
</style>