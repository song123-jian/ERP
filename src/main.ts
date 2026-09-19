import { createApp, h } from 'vue'
import { NConfigProvider, NDialogProvider, NMessageProvider, lightTheme } from 'naive-ui'
import App from './App.vue'
import './styles.css'
import { naiveTheme } from './theme'

const Root = {
  render: () => h(NConfigProvider, { theme: lightTheme, themeOverrides: naiveTheme }, {
    default: () => h(NMessageProvider, null, {
      default: () => h(NDialogProvider, null, { default: () => h(App) }),
    }),
  }),
}

createApp(Root).mount('#app')
