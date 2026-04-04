<script setup lang="ts">

</script>

<template>
  <v-navigation-drawer rail permanent class="sidenav" rail-width="58">
    <v-list class="nav-list" density="compact" nav>
      <v-list-item color="primary" prepend-icon="mdi-home" to="/"></v-list-item>
      <v-list-item color="primary" prepend-icon="mdi-tune-vertical-variant" to="/tune"></v-list-item>
      <v-list-item color="primary" prepend-icon="mdi-folder" to="/files"></v-list-item>
      <v-list-item color="primary" prepend-icon="mdi-cog" to="/settings"></v-list-item>
      <v-list-item color="primary" prepend-icon="mdi-star"></v-list-item>
    </v-list>
  </v-navigation-drawer>
</template>

<style scoped>
.nav-list {
  min-height: 100vh;
  display: flex;
  flex-direction: column;
  justify-content: space-evenly;
  margin: 0;
}
.sidenav {
}
</style>