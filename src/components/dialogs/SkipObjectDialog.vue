<script setup lang="ts">
import { computed, ref } from 'vue'
import { storeToRefs } from 'pinia'
import { useI18n } from 'vue-i18n'
import { moonraker as moonrakerClient } from '@/plugins/moonraker'
import { useAppStore } from '@/stores/app'

type ExcludeObjectDef = {
  name?: string
}

const { t } = useI18n()
const appStore = useAppStore()
const { moonraker } = storeToRefs(appStore)

const props = defineProps<{
  modelValue: boolean
}>()

const emit = defineEmits<{
  (e: 'update:modelValue', value: boolean): void
}>()

const loading = ref(false)

const dialogOpen = computed({
  get: () => props.modelValue,
  set: (value: boolean) => emit('update:modelValue', value),
})

const excludeState = computed(() => {
  const raw = moonraker.value.rawObjects['exclude_object']
  return raw && typeof raw === 'object' && !Array.isArray(raw)
      ? (raw as Record<string, unknown>)
      : {}
})

const excludedNames = computed(() => {
  const excluded = excludeState.value.excluded_objects
  return Array.isArray(excluded) ? excluded.map(String) : []
})

const currentObject = computed(() => {
  const value = excludeState.value.current_object
  return typeof value === 'string' ? value : ''
})

const objectItems = computed(() => {
  const objects = excludeState.value.objects
  if (!Array.isArray(objects)) return []

  return (objects as ExcludeObjectDef[])
      .map((item) => String(item?.name ?? ''))
      .filter(Boolean)
      .map((name) => ({
        name,
        excluded: excludedNames.value.includes(name),
        current: currentObject.value === name,
      }))
})

async function skipObject(name: string) {
  if (!name || loading.value) return

  try {
    loading.value = true
    await moonrakerClient.call('printer.gcode.script', {
      script: `EXCLUDE_OBJECT NAME=${JSON.stringify(name)}`,
    })
    dialogOpen.value = false
  } finally {
    loading.value = false
  }
}
</script>

<template>
  <v-dialog v-model="dialogOpen" max-width="700">
    <v-card rounded="lg">
      <v-card-title>{{ t('print.current.skip_object') }}</v-card-title>

      <v-card-text>
        <div v-if="!objectItems.length">
          {{ t('print.current.no_objects') }}
        </div>

        <div v-else class="skip-object-list">
          <v-card
              v-for="item in objectItems"
              :key="item.name"
              variant="tonal"
              class="skip-object-item"
          >
            <div class="skip-object-item__content">
              <div>
                <div class="skip-object-item__name">{{ item.name }}</div>
                <div class="skip-object-item__meta">
                  <span v-if="item.current">{{ t('print.current.current_object') }}</span>
                  <span v-else-if="item.excluded">{{ t('print.current.already_skipped') }}</span>
                </div>
              </div>

              <v-btn
                  color="warning"
                  variant="flat"
                  :disabled="loading || item.excluded"
                  @click="skipObject(item.name)"
              >
                {{ t('print.current.skip') }}
              </v-btn>
            </div>
          </v-card>
        </div>
      </v-card-text>

      <v-card-actions>
        <v-spacer />
        <v-btn variant="text" @click="dialogOpen = false">
          {{ t('print.current.close') }}
        </v-btn>
      </v-card-actions>
    </v-card>
  </v-dialog>
</template>

<style scoped>
.skip-object-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.skip-object-item__content {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
  padding: 12px;
}

.skip-object-item__name {
  font-weight: 700;
  word-break: break-word;
}

.skip-object-item__meta {
  font-size: 0.85rem;
  opacity: 0.8;
}
</style>