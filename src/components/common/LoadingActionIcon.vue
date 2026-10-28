<script setup lang="ts">
withDefaults(defineProps<{
  loading?: boolean
  icon: string
}>(), { loading: false })
</script>

<template>
  <v-icon
    :icon="loading ? 'mdi-loading' : icon"
    :class="{ 'loading-action-icon--spinning': loading }"
  />
</template>

<style scoped>
.loading-action-icon--spinning {
  animation: loading-action-icon-spin 1s linear infinite;
}

@keyframes loading-action-icon-spin {
  to { transform: rotate(360deg); }
}

@media (prefers-reduced-motion: reduce) {
  .loading-action-icon--spinning { animation-duration: 2s; }
}
</style>
