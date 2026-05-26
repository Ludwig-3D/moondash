<script setup lang="ts">
import WifiNetworkPanel from './network/WifiNetworkPanel.vue'
import WiredNetworkPanel from './network/WiredNetworkPanel.vue'
import WebPanelQrPanel from './network/WebPanelQrPanel.vue'
import CanbusNetworkPanel from './network/CanbusNetworkPanel.vue'
</script>

<template>
  <div class="network-panel">
    <section class="network-panel__column">
      <WifiNetworkPanel />
      <WiredNetworkPanel />
    </section>

    <section class="network-panel__column">
      <WebPanelQrPanel />
      <CanbusNetworkPanel />
    </section>
  </div>
</template>

<style scoped>
.network-panel {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 16px;
  width: 100%;
  max-height: 100%;
}

.network-panel__column {
  display: flex;
  flex-direction: column;
  gap: 16px;
  min-width: 0;
}
</style>