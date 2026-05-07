<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch, watchEffect } from 'vue'
import { useI18n } from 'vue-i18n'
import { useTheme } from 'vuetify'
import { useRouter } from 'vue-router'
import Navigation from './components/Navigation.vue'
import { useAppStore } from './stores/app'
import { moonraker } from './plugins/moonraker'
import { resolveLocale } from './plugins/i18n'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { invoke } from '@tauri-apps/api/core'
import IdleOverlay from '@/components/IdleOverlay.vue'
import LaneDialogAFC from '@/components/afc/LaneDialogAFC.vue'

const appStore = useAppStore()
const { locale } = useI18n({ useScope: 'global' })
const theme = useTheme()
const router = useRouter()


type AfcLaneDialogData = {
  id: string
  label: string
  color: string
  material: string
  weight: number | null
}

type AfcLaneState = AfcLaneDialogData & {
  hasFilament: boolean
  hasConfiguredColor: boolean
}


const primaryColor = computed(() => appStore.getPrimaryColor)
const secondaryColor = computed(() => appStore.getSecondaryColor)

const afcLaneDialogOpen = ref(false)
const selectedAfcLaneForDialog = ref<AfcLaneDialogData | null>(null)
const previousAfcLaneFilamentState = ref<Record<string, boolean> | null>(null)
const pendingAfcLaneDialogIds = ref(new Set<string>())
const previousPrintState = ref<string | null>(null)



function normalizeAfcColor(color: unknown): string {
  if (typeof color !== 'string' || !color.trim()) {
    return '#434343'
  }

  const value = color.trim()

  if (/^#[0-9a-f]{3}([0-9a-f]{3})?$/i.test(value)) {
    return value.toUpperCase()
  }

  if (/^[0-9a-f]{3}([0-9a-f]{3})?$/i.test(value)) {
    return `#${value.toUpperCase()}`
  }

  return '#434343'
}


function afcLaneHasConfiguredColor(lane: Record<string, unknown>): boolean {
  if (typeof lane.color !== 'string') return false

  const value = lane.color.trim()

  if (!value) return false

  return (
      /^#[0-9a-f]{3}([0-9a-f]{3})?$/i.test(value) ||
      /^[0-9a-f]{3}([0-9a-f]{3})?$/i.test(value)
  )
}

function afcLaneHasFilament(lane: Record<string, unknown>): boolean {
  return Boolean(
      lane.load ||
      lane.prep ||
      lane.loaded_to_hub ||
      lane.tool_loaded
  )
}

function getAfcLaneMaterial(lane: Record<string, unknown>): string {
  return typeof lane.material === 'string' && lane.material.trim()
      ? lane.material
      : ''
}

function getAfcLaneWeight(lane: Record<string, unknown>): number | null {
  return typeof lane.weight === 'number' && Number.isFinite(lane.weight)
      ? lane.weight
      : null
}



function afcLaneFilamentMetadataIsLoaded(lane: AfcLaneState): boolean {
  return Boolean(
      lane.hasFilament &&
      lane.material.trim() &&
      lane.weight !== null
  )
}

function isPrintActive(): boolean {
  const state = appStore.moonraker.printStats.state
  const normalizedState = typeof state === 'string' ? state.toLowerCase() : ''

  return ['printing', 'paused'].includes(normalizedState)
}

async function unsleepDisplay() {
  try {
    await invoke('turn_on_displays')
  } catch (err) {
    console.warn('failed to unsleep display:', err)
  }
}

async function openAfcLaneDialog(lane: AfcLaneState) {
  await unsleepDisplay()
  selectedAfcLaneForDialog.value = {
    id: lane.id,
    label: lane.label,
    color: lane.color,
    material: lane.material,
    weight: lane.weight,
  }
  afcLaneDialogOpen.value = true
}

const afcLaneStates = computed<AfcLaneState[]>(() => {
  const afc = appStore.moonraker.afc
  if (!afc.available) return []

  const objects = afc.objects as Record<string, any>
  const afcRoot = objects.AFC

  if (!afcRoot || !Array.isArray(afcRoot.units)) {
    return []
  }

  const laneNames = new Set<string>()

  for (const unitName of afcRoot.units) {
    const unitObject = objects[`AFC_BoxTurtle ${unitName}`]

    if (Array.isArray(unitObject?.lanes)) {
      for (const laneName of unitObject.lanes) {
        if (typeof laneName === 'string' && laneName.trim()) {
          laneNames.add(laneName)
        }
      }
    }
  }

  if (laneNames.size === 0 && Array.isArray(afcRoot.lanes)) {
    for (const laneName of afcRoot.lanes) {
      if (typeof laneName === 'string' && laneName.trim()) {
        laneNames.add(laneName)
      }
    }
  }

  return [...laneNames].map((laneName) => {
    const laneObject = objects[`AFC_stepper ${laneName}`] ?? {}
    const hasFilament = afcLaneHasFilament(laneObject)
    const hasConfiguredColor = afcLaneHasConfiguredColor(laneObject)
    return {
      id: laneName,
      label: laneName,
      color: hasFilament && hasConfiguredColor ? normalizeAfcColor(laneObject.color) : '#434343',
      material: getAfcLaneMaterial(laneObject),
      weight: getAfcLaneWeight(laneObject),
      hasFilament,
      hasConfiguredColor,
    }
  })
})

const afcLaneDialogColors = computed(() => {
  return afcLaneStates.value.map((lane) => lane.color)
})

watch(
    afcLaneStates,
    async (lanes) => {
      const nextFilamentState = Object.fromEntries(
          lanes.map((lane) => [lane.id, lane.hasFilament]),
      )

      if (previousAfcLaneFilamentState.value === null) {
        previousAfcLaneFilamentState.value = nextFilamentState
        return
      }

      for (const lane of lanes) {
        const wasLoaded = previousAfcLaneFilamentState.value?.[lane.id]
        const isNewlyLoaded = wasLoaded === false && lane.hasFilament

        if (isNewlyLoaded && !isPrintActive()) {
          pendingAfcLaneDialogIds.value.add(lane.id)
        }

        if (!lane.hasFilament) {
          pendingAfcLaneDialogIds.value.delete(lane.id)
        }
      }

      const laneReadyForDialog = lanes.find((lane) => {
        return (
            pendingAfcLaneDialogIds.value.has(lane.id) &&
            lane.hasFilament &&
            !lane.hasConfiguredColor &&
            afcLaneFilamentMetadataIsLoaded(lane) &&
            !isPrintActive()
        )
      })

      previousAfcLaneFilamentState.value = nextFilamentState

      if (!laneReadyForDialog) return

      pendingAfcLaneDialogIds.value.delete(laneReadyForDialog.id)
      await openAfcLaneDialog(laneReadyForDialog)
    },
    { immediate: true },
)


watch(
    () => appStore.moonraker.printStats.state,
    async (state) => {
      const normalizedState = typeof state === 'string' ? state.toLowerCase() : ''
      const wasPrinting = previousPrintState.value === 'printing'
      const isPrinting = normalizedState === 'printing'

      previousPrintState.value = normalizedState

      if (!isPrinting || wasPrinting) return

      await unsleepDisplay()

      if (router.currentRoute.value.path !== '/') {
        await router.push('/')
      }
    },
    { immediate: true },
)

const defaultLightPrimary =
    theme.themes.value.light?.colors?.primary ?? '#1976D2'
const defaultLightSecondary =
    theme.themes.value.light?.colors?.secondary ?? '#424242'
const defaultDarkPrimary =
    theme.themes.value.dark?.colors?.primary ?? '#2196F3'
const defaultDarkSecondary =
    theme.themes.value.dark?.colors?.secondary ?? '#424242'

watchEffect(() => {
  const lightTheme = theme.themes.value.light
  const darkTheme = theme.themes.value.dark

  if (lightTheme?.colors) {
    lightTheme.colors.primary = primaryColor.value || defaultLightPrimary
    lightTheme.colors.secondary = secondaryColor.value || defaultLightSecondary
  }

  if (darkTheme?.colors) {
    darkTheme.colors.primary = primaryColor.value || defaultDarkPrimary
    darkTheme.colors.secondary = secondaryColor.value || defaultDarkSecondary
  }
})

watchEffect(() => {
  let style = document.getElementById('moondash-custom-css') as HTMLStyleElement | null

  if (!style) {
    style = document.createElement('style')
    style.id = 'moondash-custom-css'
    document.head.appendChild(style)
  }

  style.textContent = appStore.getThemeCss
})

watch(
    () => appStore.getLanguage,
    (value) => {
      locale.value = resolveLocale(value)
    },
    { immediate: true },
)

onMounted(async () => {
  try {
    await appStore.startConfigListener()
    await appStore.startThemeListener()
    await appStore.loadThemeAssets()
    await appStore.loadConfig()

    locale.value = resolveLocale(appStore.getLanguage)

    await moonraker.startAutoConnectFromConfig()
  } catch (err) {
    console.error('config/moonraker init failed:', err)
  }

  await getCurrentWindow().show()
})

onBeforeUnmount(() => {
  appStore.stopConfigListener()
  appStore.stopThemeListener()
  appStore.resetConnectionState()
  moonraker.stopAutoConnectFromConfig()
  moonraker.disconnect()
})
</script>

<template>
  <v-app
      :theme="appStore.isDarkmode ? 'dark' : 'light'"
      :style="{ zoom: String(appStore.getZoom) }"
  >
    <v-layout>
      <IdleOverlay />
      <Navigation />
      <router-view />
      <LaneDialogAFC
          v-model="afcLaneDialogOpen"
          :lane="selectedAfcLaneForDialog"
          :lane-colors="afcLaneDialogColors"
      />
    </v-layout>
  </v-app>
</template>