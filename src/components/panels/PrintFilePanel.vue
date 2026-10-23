<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { moonraker as moonrakerClient } from '@/plugins/moonraker'
import PrintDialog from '@/components/dialogs/PrintDialog.vue'
import KeyboardOverlay from '../overlays/KeyboardOverlay.vue'
import PrintFilePreview from '@/components/PrintFilePreview.vue'
import GcodeDialogThumbnail from '@/components/dialogs/GcodeDialogThumbnail.vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

const { t } = useI18n()

type SortMode = 'modified-desc' | 'modified-asc' | 'name-asc' | 'name-desc'
type DisplayProfileName = 'small' | 'normal' | 'highres'

type MoonrakerFile = {
  path?: string
  filename?: string
  display?: string
  modified?: number
  permissions?: string
}

type MoonrakerThumbnail = {
  width?: number
  height?: number
  size?: number
  thumbnail_path?: string
}

type MoonrakerGcodeMetadata = {
  estimated_time?: number
  filament_weight_total?: number
  filament_weight?: number
  filament?: {
    weight_total?: number
    weight?: number
  }
}

type FolderEntry = {
  type: 'folder'
  path: string
  name: string
  modified: number
}

type FileEntry = {
  type: 'file'
  file: MoonrakerFile
  path: string
  name: string
  modified: number
}

type BrowserEntry = FolderEntry | FileEntry

type AppConfig = {
  system?: {
    display_profile?: string | null
  }
  styling?: {
    printfiles_grid_row?: number | string | null
    printfiles_grid_columns?: number | string | null
  }
}

const DISPLAY_PROFILES: Record<DisplayProfileName, { rows: number; columns: number }> = {
  small: { rows: 2, columns: 3 },
  normal: { rows: 3, columns: 4 },
  highres: { rows: 4, columns: 6 },
}


const allFiles = ref<MoonrakerFile[]>([])
const currentFolder = ref('')
const currentPage = ref(0)
const sortMode = ref<SortMode>('modified-desc')
const nameFilter = ref('')
const keyboardOpen = ref(false)
const filenameKeyboardOpen = ref(false)
const folderNameKeyboardOpen = ref(false)
const appConfig = ref<AppConfig>({})
const viewportWidth = ref(typeof window !== 'undefined' ? window.innerWidth : 0)
const viewportHeight = ref(typeof window !== 'undefined' ? window.innerHeight : 0)
let unlistenConfigLoaded: UnlistenFn | null = null

const loadingFiles = ref(false)
const loadingPage = ref(false)

const pageThumbnails = ref<Record<string, MoonrakerThumbnail[]>>({})
const fileMetadata = ref<Record<string, MoonrakerGcodeMetadata>>({})

const printDialogOpen = ref(false)
const selectedFile = ref<MoonrakerFile | null>(null)
const selectedFileMetadata = ref<MoonrakerGcodeMetadata | null>(null)

const actionsOpen = ref(false)
const moveOpen = ref(false)
const deleteOpen = ref(false)
const actionFile = ref<MoonrakerFile | null>(null)
const destinationFolder = ref('')
const destinationFilename = ref('')
const extraFolders = ref<string[]>([])
const folderPickerOpen = ref(false)
const browsingFolder = ref('')
const browsingFolders = ref<string[]>([])
const newFolderOpen = ref(false)
const newFolderName = ref('')
const createFolderBusy = ref(false)
const directoryListingBusy = ref(false)
const fileActionBusy = ref(false)
const feedbackOpen = ref(false)
const feedbackMessage = ref('')
const feedbackError = ref(false)

const actionDisplayName = computed(() => actionFile.value ? getFileName(actionFile.value) : '')
const actionPath = computed(() => actionFile.value ? getFilePath(actionFile.value) : '')

const actionMetadata = computed(() => actionFile.value ? getMetadata(actionFile.value) : null)
const actionEstimatedTime = computed(() => {
  const seconds = actionMetadata.value?.estimated_time
  if (typeof seconds !== 'number' || !Number.isFinite(seconds) || seconds < 0) return '--'
  let remaining = Math.floor(seconds)
  const days = Math.floor(remaining / 86400)
  remaining %= 86400
  const hours = Math.floor(remaining / 3600)
  remaining %= 3600
  const minutes = Math.floor(remaining / 60)
  const parts: string[] = []
  if (days > 0) parts.push(`${days}d`)
  if (hours > 0) parts.push(`${hours}h`)
  if (minutes > 0) parts.push(`${minutes}m`)
  return parts.join('') || '0m'
})
const actionFilamentWeight = computed(() => {
  const metadata = actionMetadata.value
  const weight = metadata?.filament_weight_total ?? metadata?.filament_weight ?? metadata?.filament?.weight_total ?? metadata?.filament?.weight
  return typeof weight === 'number' && Number.isFinite(weight) ? `${weight.toFixed(1)}g` : '--'
})


const visibleBrowseFolders = computed(() => browsingFolders.value.filter(name => name !== '.thumbs').sort((a, b) => a.localeCompare(b)))
const browsingFolderLabel = computed(() => browsingFolder.value || t('print_files.root_folder'))

const targetFilenameValid = computed(() => {
  const value = destinationFilename.value.trim()
  return Boolean(value) && value === destinationFilename.value && !/[\\/\\\\]/.test(value) && value !== '.' && value !== '..' && !/[\u0000-\u001f]/.test(value) && /\.gco(?:de)?$/i.test(value)
})

const destinationLabel = computed(() => destinationFolder.value || t('print_files.root_folder'))

async function browseFolder(path: string): Promise<void> {
  browsingFolder.value = path
  browsingFolders.value = []
  directoryListingBusy.value = true
  try {
    const response = await moonrakerClient.call<unknown>('server.files.get_directory', {
      path: path ? `gcodes/${path}` : 'gcodes', extended: false,
    })
    const data = response as Record<string, unknown> | null
    const dirs = data?.dirs
    const names: unknown[] = Array.isArray(dirs)
      ? dirs.map(item => typeof item === 'string' ? item : (item as Record<string, unknown>)?.dirname ?? (item as Record<string, unknown>)?.name)
      : dirs && typeof dirs === 'object' ? Object.keys(dirs) : []
    browsingFolders.value = [...new Set(names.filter((name): name is string =>
      typeof name === 'string' && !!name && !name.startsWith('.') && name !== '..' && !name.includes('/') && !name.includes('\\')
    ))]
  } catch (error) {
    console.error('Could not list folders:', error)
    notify(`${t('print_files.folder_list_failed')}: ${String(error)}`, true)
  } finally {
    directoryListingBusy.value = false
  }
}

function loadFolderPicker() {
  newFolderOpen.value = false
  folderPickerOpen.value = true
  void browseFolder(destinationFolder.value)
}

function enterBrowseFolder(name: string) {
  void browseFolder(browsingFolder.value ? `${browsingFolder.value}/${name}` : name)
}

function browseUp() {
  void browseFolder(browsingFolder.value.split('/').slice(0, -1).join('/'))
}

function chooseDestination() {
  destinationFolder.value = browsingFolder.value
  folderPickerOpen.value = false
  newFolderOpen.value = false
}

const newFolderNameValid = computed(() => {
  const name = newFolderName.value.trim()
  return Boolean(name) && name === newFolderName.value && name !== '.' && name !== '..' && !/[\/\\]/.test(name) && !/[\u0000-\u001f]/.test(name)
})

async function createFolder() {
  if (!newFolderNameValid.value || createFolderBusy.value) return
  const path = browsingFolder.value ? `${browsingFolder.value}/${newFolderName.value}` : newFolderName.value
  createFolderBusy.value = true
  try {
    await moonrakerClient.call('server.files.post_directory', { path: `gcodes/${path}` })
    browsingFolder.value = path
    destinationFolder.value = path
    newFolderName.value = ''
    newFolderOpen.value = false
    folderPickerOpen.value = false
    notify(t('print_files.folder_created'))
  } catch (error) {
    notify(`${t('print_files.folder_create_failed')}: ${String(error)}`, true)
  } finally { createFolderBusy.value = false }
}

function notify(message: string, error = false) {
  feedbackMessage.value = message
  feedbackError.value = error
  feedbackOpen.value = true
}

function showFileActions(file: MoonrakerFile) {
  actionFile.value = file
  moveOpen.value = false
  deleteOpen.value = false
  actionsOpen.value = true
}

function openMoveDialog() {
  const path = actionFile.value && getFilePath(actionFile.value)
  if (!path) return
  destinationFolder.value = path.split('/').slice(0, -1).join('/')
  destinationFilename.value = path.split('/').pop() || ''
  folderPickerOpen.value = false
  newFolderOpen.value = false
  moveOpen.value = true
}

function openDeleteDialog() {
  deleteOpen.value = true
}

const moveIsValid = computed(() => {
  const path = actionFile.value && getFilePath(actionFile.value)
  if (!path) return false
  if (!targetFilenameValid.value) return false
  const filename = destinationFilename.value
  const target = destinationFolder.value ? `${destinationFolder.value}/${filename}` : filename
  return target !== path && !allFiles.value.some(file => getFilePath(file) === target)
})

async function refreshAfterFileChange() {
  await loadFiles()
  if (currentPage.value > 0 && currentPage.value * filesPerRequest.value >= browserEntries.value.length) {
    currentPage.value = Math.max(0, Math.ceil(browserEntries.value.length / filesPerRequest.value) - 1)
  }
  await loadCurrentPageThumbnails()
}

async function moveFile() {
  const source = actionFile.value && getFilePath(actionFile.value)
  if (!source || !moveIsValid.value || fileActionBusy.value) return
  const filename = destinationFilename.value
  const dest = destinationFolder.value ? `${destinationFolder.value}/${filename}` : filename
  fileActionBusy.value = true
  try {
    await moonrakerClient.call('server.files.move', { source: `gcodes/${source}`, dest: `gcodes/${dest}` })
    actionsOpen.value = false
    moveOpen.value = false
    await refreshAfterFileChange()
    notify(t('print_files.moved'))
  } catch (error) {
    console.error('Failed to move G-code:', error)
    notify(`${t('print_files.move_failed')}: ${String(error)}`, true)
  } finally {
    fileActionBusy.value = false
  }
}

async function deleteFile() {
  const path = actionFile.value && getFilePath(actionFile.value)
  if (!path || fileActionBusy.value) return
  fileActionBusy.value = true
  try {
    await moonrakerClient.call('server.files.delete_file', { path: `gcodes/${path}` })
    actionsOpen.value = false
    deleteOpen.value = false
    await refreshAfterFileChange()
    notify(t('print_files.deleted'))
  } catch (error) {
    console.error('Failed to delete G-code:', error)
    notify(`${t('print_files.delete_failed')}: ${String(error)}`, true)
  } finally {
    fileActionBusy.value = false
  }
}



const automaticDisplayProfile = computed<DisplayProfileName>(() => {
  const width = viewportWidth.value
  const height = viewportHeight.value

  if (width >= 1920 && height >= 1080) return 'highres'
  if (width < 800 || height < 480) return 'small'
  return 'normal'
})

const configuredDisplayProfile = computed<DisplayProfileName | null>(() => {
  const profile = appConfig.value.system?.display_profile?.trim().toLowerCase()
  if (profile === 'small' || profile === 'normal' || profile === 'highres') return profile
  return null
})

const activeDisplayProfile = computed<DisplayProfileName>(() => {
  return configuredDisplayProfile.value ?? automaticDisplayProfile.value
})

function positiveInteger(value: number | string | null | undefined): number | null {
  if (value === null || value === undefined || value === '') return null

  const parsed = typeof value === 'number' ? value : Number(value)
  if (!Number.isFinite(parsed) || parsed <= 0) return null

  return Math.floor(parsed)
}

const gridRows = computed(() => {
  return positiveInteger(appConfig.value.styling?.printfiles_grid_row)
      ?? DISPLAY_PROFILES[activeDisplayProfile.value].rows
})

const gridColumns = computed(() => {
  return positiveInteger(appConfig.value.styling?.printfiles_grid_columns)
      ?? DISPLAY_PROFILES[activeDisplayProfile.value].columns
})

const filesPerRequest = computed(() => gridRows.value * gridColumns.value)

const gridStyle = computed(() => ({
  '--print-grid-columns': String(gridColumns.value),
  '--print-grid-rows': String(gridRows.value),
}))

const moonrakerHttpBase = computed(() => {
  const wsUrl = moonrakerClient.getStatus().url
  if (!wsUrl) return ''

  try {
    const parsed = new URL(wsUrl)
    const protocol = parsed.protocol === 'wss:' ? 'https:' : 'http:'
    return `${protocol}//${parsed.host}`
  } catch {
    return ''
  }
})

const breadcrumbs = computed(() => {
  if (!currentFolder.value) return []

  const parts = currentFolder.value.split('/').filter(Boolean)
  return parts.map((part, index) => ({
    name: part,
    path: parts.slice(0, index + 1).join('/'),
  }))
})

const browserEntries = computed<BrowserEntry[]>(() => {
  const folderMap = new Map<string, FolderEntry>()
  const files: FileEntry[] = []
  const folderPrefix = currentFolder.value ? `${currentFolder.value}/` : ''
  const normalizedFilter = nameFilter.value.trim().toLowerCase()

  for (const file of allFiles.value) {
    const path = getFilePath(file)
    if (!path) continue
    if (currentFolder.value && path !== currentFolder.value && !path.startsWith(folderPrefix)) continue

    const relativePath = currentFolder.value ? path.slice(folderPrefix.length) : path
    if (!relativePath || relativePath === path && currentFolder.value && !path.startsWith(folderPrefix)) continue

    const [firstSegment, ...rest] = relativePath.split('/').filter(Boolean)
    if (!firstSegment) continue

    const modified = typeof file.modified === 'number' ? file.modified : 0

    if (rest.length) {
      if (normalizedFilter && !firstSegment.toLowerCase().includes(normalizedFilter)) continue

      const folderPath = folderPrefix ? `${folderPrefix}${firstSegment}` : firstSegment
      const existing = folderMap.get(folderPath)
      if (!existing || modified > existing.modified) {
        folderMap.set(folderPath, {
          type: 'folder',
          path: folderPath,
          name: firstSegment,
          modified,
        })
      }
      continue
    }

    const name = getFileName(file)
    if (normalizedFilter && !name.toLowerCase().includes(normalizedFilter)) continue

    files.push({
      type: 'file',
      file,
      path,
      name,
      modified,
    })
  }

  return [...folderMap.values(), ...files].sort(compareEntries)
})

const pageEntries = computed(() => {
  const start = currentPage.value * filesPerRequest.value
  return browserEntries.value.slice(start, start + filesPerRequest.value)
})

const pageFolders = computed(() => pageEntries.value.filter((entry): entry is FolderEntry => entry.type === 'folder'))
const pageFiles = computed(() => pageEntries.value.filter((entry): entry is FileEntry => entry.type === 'file'))
const canGoUp = computed(() => currentPage.value > 0)

const canGoDown = computed(() => {
  const nextStart = (currentPage.value + 1) * filesPerRequest.value
  return nextStart < browserEntries.value.length
})

const sortButtons = computed<Array<{ mode: SortMode; icon: string; label: string }>>(() => [
  { mode: 'modified-desc', icon: 'mdi-sort-clock-descending-outline', label: 'Modified newest first' },
  { mode: 'modified-asc', icon: 'mdi-sort-clock-ascending-outline', label: 'Modified oldest first' },
  { mode: 'name-asc', icon: 'mdi-sort-alphabetical-ascending', label: 'Name A to Z' },
  { mode: 'name-desc', icon: 'mdi-sort-alphabetical-descending', label: 'Name Z to A' },
])

function compareEntries(a: BrowserEntry, b: BrowserEntry): number {
  if (a.type !== b.type) return a.type === 'folder' ? -1 : 1

  switch (sortMode.value) {
    case 'modified-asc':
      return a.modified - b.modified || a.name.localeCompare(b.name, undefined, { sensitivity: 'base' })
    case 'name-asc':
      return a.name.localeCompare(b.name, undefined, { sensitivity: 'base' })
    case 'name-desc':
      return b.name.localeCompare(a.name, undefined, { sensitivity: 'base' })
    case 'modified-desc':
    default:
      return b.modified - a.modified || a.name.localeCompare(b.name, undefined, { sensitivity: 'base' })
  }
}

function getFileName(file: MoonrakerFile): string {
  const name = file.display || file.filename || file.path || t('print_files.unknown_file')
  const pathParts = name.split('/').filter(Boolean)
  const leafName = pathParts.length ? pathParts[pathParts.length - 1] : name
  return leafName.replace(/\.gcode$/i, '')
}

function getMetadata(file: MoonrakerFile): MoonrakerGcodeMetadata | null {
  const filePath = getFilePath(file)
  if (!filePath) return null
  return fileMetadata.value[filePath] ?? null
}

function getFilePath(file: MoonrakerFile): string | null {
  const value = file.path || file.filename || file.display
  return typeof value === 'string' && value.trim() ? value.replace(/^\/+/, '') : null
}

function getBestThumbnailUrl(file: MoonrakerFile): string | null {
  const filePath = getFilePath(file)
  if (!filePath) return null

  const thumbnails = pageThumbnails.value[filePath]
  if (!Array.isArray(thumbnails) || !thumbnails.length) return null

  const sorted = [...thumbnails].sort((a, b) => {
    const aArea = (a.width ?? 0) * (a.height ?? 0)
    const bArea = (b.width ?? 0) * (b.height ?? 0)
    return bArea - aArea
  })

  const thumbnailPath = sorted[0]?.thumbnail_path
  if (!thumbnailPath || !moonrakerHttpBase.value) return null

  return `${moonrakerHttpBase.value}/server/files/gcodes/${thumbnailPath}`
}

function openPrintDialog(file: MoonrakerFile) {
  const filePath = getFilePath(file)

  selectedFile.value = file
  selectedFileMetadata.value = filePath ? fileMetadata.value[filePath] ?? null : null
  printDialogOpen.value = true
}

async function openFolder(path: string) {
  currentFolder.value = path
  currentPage.value = 0
  await loadFiles()
  await loadCurrentPageThumbnails()
}

async function goBackFolder() {
  if (!currentFolder.value) return

  const parts = currentFolder.value.split('/').filter(Boolean)
  parts.pop()
  currentFolder.value = parts.join('/')
  currentPage.value = 0

  await loadFiles()
  await loadCurrentPageThumbnails()
}

async function goRootFolder() {
  currentFolder.value = ''
  currentPage.value = 0

  await loadFiles()
  await loadCurrentPageThumbnails()
}


function updateViewportSize() {
  viewportWidth.value = window.innerWidth
  viewportHeight.value = window.innerHeight
}

async function loadConfig() {
  try {
    appConfig.value = await invoke<AppConfig>('get_config')
  } catch (error) {
    console.error('Failed to load app config', error)
    appConfig.value = {}
  }
}

async function loadFiles() {
  try {
    loadingFiles.value = true

    const result = await moonrakerClient.call<unknown>('server.files.list')

    if (Array.isArray(result)) {
      allFiles.value = result as MoonrakerFile[]
      return
    }

    if (result && typeof result === 'object') {
      const record = result as Record<string, unknown>

      if (Array.isArray(record.files)) {
        allFiles.value = record.files as MoonrakerFile[]
        return
      }

      if (Array.isArray(record.result)) {
        allFiles.value = record.result as MoonrakerFile[]
        return
      }
    }

    allFiles.value = []
  } catch (error) {
    console.error('Failed to load print files', error)
    allFiles.value = []
  } finally {
    loadingFiles.value = false
  }
}

async function loadCurrentPageThumbnails() {
  try {
    loadingPage.value = true
    pageThumbnails.value = {}
    fileMetadata.value = {}

    const entries = await Promise.all(
        pageFiles.value.map(async (entry) => {
          const filePath = entry.path
          if (!filePath) return null

          const [thumbnailResult, metadataResult] = await Promise.all([
            moonrakerClient
                .call<MoonrakerThumbnail[]>('server.files.thumbnails', { filename: filePath })
                .catch(() => []),
            moonrakerClient
                .call<MoonrakerGcodeMetadata>('server.files.metadata', { filename: filePath })
                .catch(() => null),
          ])

          if (metadataResult) {
            fileMetadata.value[filePath] = metadataResult
          }

          return [filePath, Array.isArray(thumbnailResult) ? thumbnailResult : []] as const
        }),
    )

    pageThumbnails.value = Object.fromEntries(
        entries.filter(Boolean) as Array<readonly [string, MoonrakerThumbnail[]]>,
    )
  } finally {
    loadingPage.value = false
  }
}

async function nextPage() {
  if (!canGoDown.value || loadingFiles.value || loadingPage.value) return
  currentPage.value += 1
}

async function previousPage() {
  if (!canGoUp.value || loadingFiles.value || loadingPage.value) return
  currentPage.value -= 1
}

watch(currentPage, async () => {
  await loadCurrentPageThumbnails()
})

watch(filesPerRequest, async () => {
  currentPage.value = 0
  await loadCurrentPageThumbnails()
})

watch([currentFolder, sortMode, nameFilter], async () => {
  currentPage.value = 0
  await loadCurrentPageThumbnails()
})

onMounted(async () => {
  window.addEventListener('resize', updateViewportSize)

  unlistenConfigLoaded = await listen<AppConfig>('config-loaded', (event) => {
    appConfig.value = event.payload ?? {}
  })

  await loadConfig()
  currentPage.value = 0
  await loadFiles()
  await loadCurrentPageThumbnails()
})

onBeforeUnmount(() => {
  window.removeEventListener('resize', updateViewportSize)
  unlistenConfigLoaded?.()
})
</script>

<template>
  <div
      class="print-file-panel"
      :class="{ 'print-file-panel--subfolder': currentFolder }"
  >
    <v-toolbar
        class="print-file-panel__toolbar px-2"
        density="compact"
        rounded
        flat
    >
      <v-btn-toggle
          v-model="sortMode"
          mandatory
          density="comfortable"
          divided
          color="primary"
      >
        <v-btn
            v-for="button in sortButtons"
            :key="button.mode"
            :value="button.mode"
            :icon="button.icon"
            style="min-width: 50px;"
        />
      </v-btn-toggle>

      <v-text-field
          v-model="nameFilter"
          class="print-file-panel__filter"
          density="compact"
          variant="plain"
          hide-details
          readonly
          prepend-inner-icon="mdi-filter-outline"
          :placeholder="t('print_files.filter')"
          single-line
          @click="keyboardOpen = true"
          @focus="keyboardOpen = true"
      />
    </v-toolbar>

    <div v-if="currentFolder" class="print-file-panel__breadcrumbs">
      <v-btn
          icon="mdi-arrow-left"
          density="comfortable"
          variant="text"
          @click="goBackFolder"
      />
      <v-btn
          size="small"
          variant="text"
          prepend-icon="mdi-home-outline"
          @click="goRootFolder"
      >

      </v-btn>
      <template v-for="crumb in breadcrumbs" :key="crumb.path">
        <v-icon icon="mdi-chevron-right" size="16" />
        <v-btn
            size="small"
            variant="text"
            @click="openFolder(crumb.path)"
        >
          {{ crumb.name }}
        </v-btn>
      </template>
    </div>

    <div class="print-file-panel__nav">
      <v-btn
          v-if="canGoUp"
          icon="mdi-chevron-up"
          :disabled="loadingFiles || loadingPage"
          @click="previousPage"
      />

      <v-spacer />

      <v-btn
          v-if="canGoDown"
          icon="mdi-chevron-down"
          :disabled="loadingFiles || loadingPage"
          @click="nextPage"
      />
    </div>

    <div v-if="loadingFiles" class="print-file-panel__state">
      <v-progress-circular indeterminate />
    </div>

    <div v-else-if="!pageEntries.length" class="print-file-panel__state">
      <v-alert type="info" variant="tonal">
        {{ t('print_files.none_found') }}
      </v-alert>
    </div>

    <div
        v-else
        class="print-file-panel__grid"
        :style="gridStyle"
    >
      <v-card
          v-for="entry in pageFolders"
          :key="`folder:${entry.path}`"
          class="print-folder-card pa-0"
          variant="flat"
          @click="openFolder(entry.path)"
      >
        <v-icon icon="mdi-folder" style="font-size: 48px;" />
        <div class="print-folder-card__title">
          {{ entry.name }}
        </div>
      </v-card>

      <PrintFilePreview
          v-for="entry in pageFiles"
          :key="entry.path"
          :file="entry.file"
          :title="entry.name"
          :thumbnail-url="getBestThumbnailUrl(entry.file)"
          :metadata="getMetadata(entry.file)"
          :loading="loadingPage"
          @select="openPrintDialog"
          @actions="showFileActions"
      />
    </div>

    <KeyboardOverlay
        v-model="nameFilter"
        :visible="keyboardOpen"
        :title="t('print_files.filter_description')"
        @enter="keyboardOpen = false"
        @close="keyboardOpen = false"
    />

    <v-dialog v-model="folderPickerOpen" max-width="700" :persistent="createFolderBusy"
              content-class="file-folder-dialog">
      <v-card class="file-folder-card" rounded="lg">
        <div class="file-folder-header">
          <div class="file-folder-title">
            <v-icon icon="mdi-folder-open-outline" size="26" />
            <span>{{ t('print_files.destination') }}</span>
          </div>
        </div>

        <div class="file-folder-content">
          <div class="file-folder-location">
            <v-btn icon="mdi-arrow-up" variant="tonal" size="small"
                   :disabled="!browsingFolder || directoryListingBusy" @click="browseUp" />
            <span class="text-truncate">{{ browsingFolderLabel }}</span>
          </div>
          <v-progress-linear v-if="directoryListingBusy" indeterminate class="file-folder-progress" />

          <div class="file-folder-scroll">
            <v-list bg-color="transparent" class="pa-0">
              <v-list-item v-for="name in visibleBrowseFolders" :key="name"
                           class="file-action-folder-row" :disabled="directoryListingBusy"
                           @click="enterBrowseFolder(name)">
                <template #prepend><v-icon icon="mdi-folder-outline" size="27" /></template>
                <v-list-item-title>{{ name }}</v-list-item-title>
                <template #append><v-icon icon="mdi-chevron-right" /></template>
              </v-list-item>
            </v-list>
          </div>


        </div>

        <v-card-actions class="file-folder-footer">
          <template v-if="newFolderOpen">
            <v-text-field v-model="newFolderName" :label="t('print_files.new_folder_name')"
                          prepend-inner-icon="mdi-folder-plus-outline" variant="outlined"
                          density="comfortable" hide-details readonly class="file-folder-new-name"
                          :disabled="createFolderBusy" @click="folderNameKeyboardOpen = true"
                          @focus="folderNameKeyboardOpen = true" />
            <v-spacer />
            <v-btn variant="text" size="large" :disabled="createFolderBusy"
                   @click="newFolderOpen = false; newFolderName = ''">{{ t('print_files.cancel') }}</v-btn>
            <v-btn color="primary" variant="text" size="large"
                   :disabled="!newFolderNameValid" :loading="createFolderBusy"
                   @click="createFolder">{{ t('print_files.create_folder') }}</v-btn>
          </template>
          <template v-else>
            <v-spacer />
            <v-btn variant="text" size="large" :disabled="createFolderBusy"
                   @click="folderPickerOpen = false">{{ t('print_files.cancel') }}</v-btn>
            <v-btn variant="text" size="large" prepend-icon="mdi-folder-plus-outline"
                   :disabled="directoryListingBusy" @click="newFolderOpen = true">
              {{ t('print_files.create_folder') }}
            </v-btn>
            <v-btn color="primary" variant="text" size="large" :disabled="directoryListingBusy || createFolderBusy"
                   @click="chooseDestination">{{ t('print_files.select_this_folder') }}</v-btn>
          </template>
        </v-card-actions>
      </v-card>
    </v-dialog>

    <KeyboardOverlay v-model="destinationFilename" :visible="filenameKeyboardOpen"
                     :title="t('print_files.filename')" @enter="filenameKeyboardOpen = false" @close="filenameKeyboardOpen = false" />
    <KeyboardOverlay v-model="newFolderName" :visible="folderNameKeyboardOpen"
                     :title="t('print_files.new_folder_name')" @enter="folderNameKeyboardOpen = false" @close="folderNameKeyboardOpen = false" />

    <!-- Same image-left, controls-right layout used by PrintDialog. -->
    <v-dialog v-model="actionsOpen" max-width="1200" :persistent="fileActionBusy">
      <v-card v-if="actionFile" rounded="lg" class="pa-0">
        <v-card-text class="pa-0">
          <div class="file-action-layout">
            <div class="file-action-preview">
              <GcodeDialogThumbnail :file-path="actionPath" class="file-action-thumb" />
              <div class="file-action-info">
                <div class="file-action-name">{{ actionDisplayName }}</div>
                <div class="file-action-meta">
                  <span class="file-action-meta-item"><v-icon icon="mdi-clock-outline" size="16" />{{ actionEstimatedTime }}</span>
                  <span class="file-action-meta-item"><v-icon icon="mdi-scale-balance" size="16" />{{ actionFilamentWeight }}</span>
                </div>
              </div>
            </div>

            <div class="file-action-controls pa-2">
              <template v-if="moveOpen">
                <div class="file-action-heading">
                  <v-icon icon="mdi-folder-move-outline" />
                  {{ t('print_files.move') }}
                </div>
                <div class="file-action-move-content mt-4">
                  <div class="file-action-field-label">{{ t('print_files.destination') }}</div>
                  <v-btn class="file-action-folder-trigger" variant="outlined" block
                         :disabled="fileActionBusy" @click="loadFolderPicker">
                    <v-icon icon="mdi-folder-outline" size="24" class="mr-3" />
                    <span class="file-action-destination-name">{{ destinationLabel }}</span>
                    <v-icon icon="mdi-chevron-right" class="ml-auto" />
                  </v-btn>
                  <v-text-field v-model="destinationFilename" :label="t('print_files.filename')"
                                prepend-inner-icon="mdi-file-code-outline" variant="outlined"
                                density="comfortable" class="mt-4" hide-details readonly
                                :disabled="fileActionBusy" @click="filenameKeyboardOpen = true" @focus="filenameKeyboardOpen = true" />
                  <div v-if="!targetFilenameValid" class="file-action-hint mt-2">
                    <v-icon icon="mdi-information-outline" size="18" />
                    <span>{{ t('print_files.invalid_filename') }}</span>
                  </div>
                  <div v-else-if="!moveIsValid" class="file-action-hint mt-2">
                    <v-icon icon="mdi-information-outline" size="18" />
                    <span>{{ t('print_files.choose_other_folder') }}</span>
                  </div>
                </div>
              </template>
              <template v-else-if="deleteOpen">
                <div class="file-action-heading">
                  <v-icon icon="mdi-delete-outline" color="error" />
                  {{ t('print_files.delete_confirm_title') }}
                </div>
                <v-alert class="mt-5 file-action-delete-alert" type="warning" variant="tonal" density="comfortable" icon="mdi-alert-outline">
                  {{ t('print_files.delete_confirm_message') }}
                </v-alert>
              </template>
              <template v-else>
                <div class="file-action-heading">{{ t('print_files.file_actions') }}</div>
                <div class="file-action-choices">
                  <v-btn variant="tonal" prepend-icon="mdi-folder-move-outline" size="large" block @click="openMoveDialog">
                    {{ t('print_files.move') }}
                  </v-btn>
                  <v-btn color="error" variant="tonal" prepend-icon="mdi-delete-outline" size="large" block @click="openDeleteDialog">
                    {{ t('print_files.delete') }}
                  </v-btn>
                </div>
              </template>

              <div class="file-action-buttons">
                <v-spacer />
                <v-btn variant="text" :disabled="fileActionBusy" @click="moveOpen || deleteOpen ? (moveOpen = false, deleteOpen = false) : (actionsOpen = false)">
                  {{ t('print_files.cancel') }}
                </v-btn>
                <v-btn v-if="moveOpen" variant="text" color="primary" :loading="fileActionBusy" :disabled="!moveIsValid" @click="moveFile">
                  {{ t('print_files.move') }}
                </v-btn>
                <v-btn v-else-if="deleteOpen" variant="text" color="error" :loading="fileActionBusy" @click="deleteFile">
                  {{ t('print_files.delete') }}
                </v-btn>
              </div>
            </div>
          </div>
        </v-card-text>
      </v-card>
    </v-dialog>

    <v-snackbar v-model="feedbackOpen" :color="feedbackError ? 'error' : 'success'" timeout="4000">
      {{ feedbackMessage }}
      <template #actions><v-btn variant="text" @click="feedbackOpen = false">{{ t('print_files.close') }}</v-btn></template>
    </v-snackbar>

    <PrintDialog
        v-model="printDialogOpen"
        :file="selectedFile"
        :metadata="selectedFileMetadata"
        @start="() => {}"
    />
  </div>
</template>

<style scoped>
.print-file-panel {
  height: 100%;
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 8px;
  box-sizing: border-box;
}

.print-file-panel__toolbar {
  display: flex;
  align-items: center;
  gap: 10px;
}

.print-file-panel__filter {
  max-width: 320px;
  margin-left: auto;
}

.print-file-panel__breadcrumbs {
  display: flex;
  align-items: center;
  gap: 2px;
  min-height: 36px;
  overflow-x: auto;
}

.print-file-panel__nav {
  z-index: 2;
  position: fixed;
  right: 8px;
  bottom: 8px;
  display: flex;
  align-items: center;
  flex-direction: column;
  gap: 8px;
}

.print-file-panel__state {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
}

.print-file-panel__grid {
  flex: 1;
  display: grid;
  grid-template-columns: repeat(var(--print-grid-columns), minmax(0, 1fr));
  grid-template-rows: repeat(var(--print-grid-rows), minmax(0, 1fr));
  gap: 16px;
  min-height: 0;
  overflow: auto;
}

.print-folder-card {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  cursor: pointer;
  box-shadow: none !important;
  height: 100%;
  min-height: 0;
}

.print-folder-card__title {
  max-width: 100%;
  font-weight: 600;
  line-height: 1.2;
  text-align: center;
  word-break: break-word;
}

.print-file-panel__filter :deep(.v-field) {
  align-items: center;
}

.print-file-panel__filter :deep(.v-field__field) {
  align-items: center;
}

.print-file-panel__filter :deep(.v-field__input) {
  min-height: 36px;
  padding-top: 0;
  padding-bottom: 0;
  align-items: center;
}

.print-file-panel__filter :deep(.v-field__prepend-inner) {
  transform: translateY(-4px);
}
</style>

<style scoped>
/* Match PrintDialog's grid, image, info overlay, and right-hand action rail. */
.file-action-layout {
  display: grid;
  grid-template-columns: minmax(360px, 1.25fr) minmax(360px, 1fr);
  align-items: stretch;
}
.file-action-preview { position: relative; display: flex; flex-direction: column; height: 100%; min-height: 0; }
.file-action-thumb { flex: 1; min-height: 360px; height: 100%; overflow: hidden; }
.file-action-info {
  position: absolute; bottom: 0; left: 0; right: 0;
  padding: 12px 14px; background: rgba(var(--v-theme-background), .8);
  display: flex; flex-direction: column; gap: 8px;
}
.file-action-name { font-size: 1rem; font-weight: 700; line-height: 1.2; word-break: break-word; }
.file-action-meta { display: flex; flex-wrap: wrap; gap: 12px; opacity: .85; }
.file-action-meta-item { display: inline-flex; align-items: center; gap: 6px; }
.file-action-controls {
  display: flex; flex-direction: column; min-width: 0; min-height: 0;
  height: 100%; background: rgba(var(--v-theme-on-surface), .08);
}
.file-action-heading { font-size: 1.1rem; font-weight: 600; display: flex; align-items: center; gap: 10px; }
.file-action-choices { display: flex; flex-direction: column; gap: 14px; margin-top: 24px; }
.file-action-move-content { width: 100%; flex: 0 0 auto; min-height: 0; }
.file-action-field-label { font-size: .85rem; opacity: .8; margin-bottom: 8px; }
.file-action-folder-trigger { height: 48px; min-height: 48px; justify-content: flex-start; text-transform: none; padding: 0 14px; }
.file-action-destination-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; text-align: left; }

.file-action-folder-row { min-height: 62px; border-radius: 8px; margin: 2px 0; }
.file-action-hint { display: flex; align-items: flex-start; gap: 9px; font-size: .875rem; opacity: .75; line-height: 1.4; }
.file-action-hint .v-icon { flex-shrink: 0; }
.file-action-delete-alert { flex: 0 0 auto; height: auto; min-height: 0; }
.file-action-delete-warning {
  display: flex; align-items: flex-start; gap: 12px;
  padding: 14px 12px; border-radius: 8px;
  background: rgba(var(--v-theme-on-surface), .06);
  line-height: 1.5;
}
.file-action-delete-warning .v-icon { flex-shrink: 0; }

.file-action-buttons {
  margin-top: auto; display: flex; justify-content: flex-end;
  align-items: center; gap: 12px; padding-top: 18px;
}
@media (max-width: 700px) {
  .file-action-layout { grid-template-columns: 1fr; }
  .file-action-preview { min-height: 180px; }
  .file-action-thumb { min-height: 180px; }
  .file-action-controls { min-height: 220px; }
}
</style>

<style scoped>
/* Keep the folder picker within the viewport: only its directory list scrolls. */
.file-folder-card {
  display: flex;
  flex-direction: column;
  height: min(580px, 82dvh);
  max-height: 82dvh;
  min-height: 0;
  overflow: hidden;
}
.file-folder-header {
  flex: 0 0 auto;
  padding: 16px 20px 12px;
}
.file-folder-title {
  display: flex;
  align-items: center;
  gap: 12px;
  font-size: 1.15rem;
  font-weight: 600;
}
.file-folder-content {
  display: flex;
  flex: 1 1 auto;
  flex-direction: column;
  min-height: 0;
  padding: 0 20px;
}
.file-folder-location {
  flex: 0 0 auto;
  min-height: 64px;
  display: flex;
  align-items: center;
  gap: 12px;
}
.file-folder-location > span { min-width: 0; font-size: 1rem; }
.file-folder-progress { flex: 0 0 auto; }
.file-folder-scroll {
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
}
.file-action-folder-row { min-height: 64px; }
.file-folder-footer {
  flex: 0 0 auto;
  display: flex;
  gap: 8px;
  padding: 8px 16px 12px;
  flex-wrap: wrap;
}
.file-folder-new-name { min-width: 180px; flex: 1 1 220px; }
</style>
