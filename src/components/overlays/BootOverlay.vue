<script setup lang="ts">
import {useI18n} from "vue-i18n";

const { t } = useI18n()
</script>

<template>
  <div class="overlay-container d-flex flex-column align-center justify-center text-center">
    <div>
      <span class="text-primary">{{ t('boot.startup') }}</span>
      <v-progress-linear indeterminate color="primary" class="mt-2"></v-progress-linear>
    </div>
  </div>
</template>

<style scoped>

</style>