<script setup lang="ts">
import { computed, ref } from 'vue'
import { storeToRefs } from 'pinia'
import { useAppStore } from '@/stores/app'
import ShortcutBar from '../components/ShortcutBar.vue'
import NotificationPanel from '../components/panels/NotificationPanel.vue'
import CurrentPrintPanel from '../components/panels/CurrentPrintPanel.vue'
import JobQueuePanel from '../components/panels/JobQueuePanel.vue'

const appStore = useAppStore()
const { moonraker } = storeToRefs(appStore)

const hasNotifications = ref(false)
const hasJobQueue = ref(false)

const isPrinting = computed(() => moonraker.value.printStats?.state === 'printing')
const showJobQueuePanel = computed(() => hasJobQueue.value && !hasNotifications.value && !isPrinting.value)
</script>

<template>
  <v-main>
    <v-row class="home-layout" no-gutters no-wrap>
      <v-col class="pt-2 pb-2 pr-2">
        <CurrentPrintPanel />
      </v-col>

      <v-col cols="auto" class="home-layout__sidebar pr-2">
        <ShortcutBar />
      </v-col>

      <v-col
          v-show="hasNotifications || showJobQueuePanel"
          cols="auto"
          class="home-layout__panel"
      >
        <NotificationPanel @active-change="hasNotifications = $event" />
        <JobQueuePanel
            v-show="showJobQueuePanel"
            @active-change="hasJobQueue = $event"
        />
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
