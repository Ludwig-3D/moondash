<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { moonraker as moonrakerClient } from '@/plugins/moonraker'

/** Independent G-code preview for dialogs; never relies on the paginated file grid cache. */
const props = defineProps<{ filePath: string }>()
type Thumbnail = { width?: number; height?: number; thumbnail_path?: string }
const thumbnails = ref<Thumbnail[]>([])
const loading = ref(false)
const imageFailed = ref(false)
let requestId = 0

function normalize(path: string) {
  return path.trim().replace(/^\/+/, '').replace(/^gcodes\//, '').replace(/^\.\//, '')
}

function resolvePath(filePath: string, thumbnailPath: string) {
  const file = normalize(filePath)
  const thumb = normalize(thumbnailPath)
  const dir = file.includes('/') ? file.slice(0, file.lastIndexOf('/')) : ''
  if (!dir || thumb.startsWith(`${dir}/`) || thumb.startsWith(`.thumbs/${dir}/`)) return thumb
  return `${dir}/${thumb}`
}

const thumbnailUrl = computed(() => {
  const path = props.filePath
  const thumb = [...thumbnails.value]
    .sort((a, b) => (b.width ?? 0) * (b.height ?? 0) - (a.width ?? 0) * (a.height ?? 0))
    .find(item => item.thumbnail_path)?.thumbnail_path
  const wsUrl = moonrakerClient.getStatus().url
  if (!path || !thumb || !wsUrl) return null
  try {
    const url = new URL(wsUrl)
    const protocol = url.protocol === 'wss:' ? 'https:' : 'http:'
    const resolved = resolvePath(path, thumb).split('/').map(encodeURIComponent).join('/')
    return `${protocol}//${url.host}/server/files/gcodes/${resolved}`
  } catch {
    return null
  }
})

watch(() => props.filePath, async (path) => {
  const id = ++requestId
  thumbnails.value = []
  imageFailed.value = false
  loading.value = !!path
  if (!path) return
  try {
    const result = await moonrakerClient.call<unknown>('server.files.thumbnails', { filename: path })
    if (id !== requestId) return
    if (Array.isArray(result)) thumbnails.value = result as Thumbnail[]
    else if (result && typeof result === 'object') {
      const record = result as Record<string, unknown>
      const items = Array.isArray(record.thumbnails) ? record.thumbnails : record.result
      thumbnails.value = Array.isArray(items) ? items as Thumbnail[] : []
    }
  } catch (error) {
    if (id === requestId) console.warn('Could not load G-code dialog thumbnail:', error)
  } finally {
    if (id === requestId) loading.value = false
  }
}, { immediate: true })
</script>

<template>
  <div class="gcode-dialog-thumbnail">
    <img v-if="thumbnailUrl && !imageFailed" :src="thumbnailUrl" alt="G-code thumbnail" @error="imageFailed = true" />
    <div v-if="!thumbnailUrl || imageFailed" class="gcode-dialog-thumbnail__placeholder">
      <v-progress-circular v-if="loading" indeterminate size="40" />
      <v-icon v-else icon="mdi-printer-3d" size="72" />
    </div>
  </div>
</template>

<style scoped>
.gcode-dialog-thumbnail { position: relative; display: flex; align-items: center; justify-content: center; overflow: hidden; }
.gcode-dialog-thumbnail img { width: 100%; height: 100%; position: absolute; inset: 0; object-fit: cover; }
.gcode-dialog-thumbnail__placeholder { display: flex; align-items: center; justify-content: center; opacity: .6; }
</style>
