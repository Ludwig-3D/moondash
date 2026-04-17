import { createApp } from "vue";
import App from "./App.vue";
import vuetify from './plugins/vuetify'
import router from '@/router'
import {createPinia} from "pinia";
import { i18n } from './plugins/i18n'

const app = createApp(App)

app
    .use(createPinia())
    .use(vuetify)
    .use(router)
    .use(i18n)

app.mount("#app");
