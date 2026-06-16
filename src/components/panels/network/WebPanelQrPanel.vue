<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import QRCode from 'qrcode'
import { useAppStore } from '@/stores/app'

const { t } = useI18n()
const appStore = useAppStore()

const qrDataUrl = ref<string | null>(null)
const loading = ref(false)

const primaryIp = computed(() => appStore.getPrimaryIp)
const error = computed(() => appStore.getPrimaryIpError)

const webPanelUrl = computed(() => {
  if (!primaryIp.value) return null
  return `http://${primaryIp.value}`
})

async function generateQrForIp(nextPrimaryIp: string) {
  const nextWebPanelUrl = `http://${nextPrimaryIp}`

  qrDataUrl.value = await QRCode.toDataURL(nextWebPanelUrl, {
    margin: 1,
    width: 200,
  })
}

watch(
    primaryIp,
    async (nextPrimaryIp) => {
      if (!nextPrimaryIp) {
        qrDataUrl.value = null
        return
      }

      loading.value = true

      try {
        await generateQrForIp(nextPrimaryIp)
      } finally {
        loading.value = false
      }
    },
    { immediate: true },
)
</script>

<template>
  <v-card rounded="lg" variant="flat">

    <v-card-text class="webpanel-qr pa-0">
      <v-alert
          v-if="error"
          type="error"
          variant="tonal"
          density="compact"
      >
        {{ error }}
      </v-alert>

      <v-progress-circular
          v-if="loading && !qrDataUrl"
          indeterminate
          color="primary"
      />

      <template v-else-if="webPanelUrl && qrDataUrl">
        <img
            :src="qrDataUrl"
            alt="Webpanel QR Code"
            class="webpanel-qr__image"
        >

        <div class="webpanel-qr__content">
          <div class="webpanel-qr__label">
            {{ t('settings.network.webpanel') }}
          </div>

          <div class="webpanel-qr__url">
            {{ webPanelUrl }}
          </div>
        </div>
      </template>

      <v-alert
          v-else-if="!error"
          type="info"
          variant="tonal"
          density="compact"
      >
        {{ t('settings.network.none') }}
      </v-alert>
    </v-card-text>
  </v-card>
</template>

<style scoped>
.webpanel-qr {
  display: flex;
  align-items: center;
  gap: 16px;
}

.webpanel-qr__image {
  flex: 0 0 auto;
  width: 160px;
  height: 160px;
  border-radius: 8px;
}

.webpanel-qr__content {
  padding-top: 5px;
  min-width: 0;
  flex: 1;
  align-self: stretch;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  justify-content: flex-start;
}

.webpanel-qr__label {
  font-size: 1.25rem;
  font-weight: 500;
  line-height: 2rem;
  letter-spacing: 0.0125em;
}

.webpanel-qr__url {
  margin-top: -2px;
  font-size: 0.875rem;
  font-weight: 400;
  line-height: 1.25rem;
  opacity: 0.7;
  word-break: break-all;
}
</style>