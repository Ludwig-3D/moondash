<script setup lang="ts">
import ShortcutBar from '../components/ShortcutBar.vue'
import NotificationPanel from '../components/panels/NotificationPanel.vue'
import CurrentPrintPanel from "../components/panels/CurrentPrintPanel.vue";
</script>

<template>
  <v-main>
    <v-row class="home-layout" no-gutters no-wrap>
      <v-col class="pt-2 pb-2 pr-2">
        <CurrentPrintPanel/>
      </v-col>

      <v-col cols="auto" class="home-layout__sidebar pr-2">
        <ShortcutBar />
      </v-col>

      <v-col cols="auto" class="home-layout__panel">
        <NotificationPanel />
      </v-col>
    </v-row>
  </v-main>
</template>

<style scoped>
.home-layout {
  height: 100%;
  width: 100%;
  margin: 0;
  flex-wrap: nowrap;
  align-items: stretch;
}

.home-layout__sidebar {
  flex: 0 0 auto;
  max-width: none;
}

.home-layout__panel {
  flex: 0 1 auto;
  max-width: none;
  min-width: 0;
  height: 100%;
}
</style>