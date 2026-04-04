// Utilities
import { defineStore } from 'pinia'

export const useAppStore = defineStore('app', {
  state: () => ({
    assets: [],
    status: {},
    config: {
      websocketPort: 8100,
      webserverPort: 8105
    },
    games: [],
    alerts: [],
    websocket: {
      connected: false,
      connecting: false
    },
    shieldMode: false,
    currentGame: {},
    channelPoints: [],
    audioData: {},
    systemInfo: {
      components: {},
      config: {},
    },
    throttled: false,
    scene: {},
    connections: {},
    backendConfig: '',
    parsedBackendConfig: {},
    obsSceneData: [],
    testMode: false,
    voices: [],
    macros: {},
    autoMacros: [],
    variables: {},
    giveaway: {},
    yoloboxData: {},
    obsAudioData: {}
  }),
  getters: {
    getConfig: (state) => state.config,
    getWebsocket: (state) => {
      return `ws://${location.hostname}:${state.config.websocketPort}`
    },
    getRestApi: (state) => {
      return `http://${location.hostname}:${state.config.webserverPort}`
    },
    getGames: (state) => state.games,
    getAlerts: (state) => state.alerts,
    isWebsocketConnected: (state) => state.websocket.connected,
    isWebsocketConnecting: (state) => state.websocket.connecting,
    getCurrentGame: (state) => state.currentGame,
    isShieldActive: (state) => state.shieldMode,
    getChannelPoints: (state) => state.channelPoints,
    getAudioData: (state) => state.audioData,
    getSystemInfo: (state) => state.systemInfo,
    isThrottled: (state) => state.throttled,
    getScene: (state) => state.scene,
    getConnections: (state) => state.connections,
    getBackendConfig: (state) => state.backendConfig,
    getParsedBackendConfig: (state) => state.parsedBackendConfig,
    getObsSceneData: (state) => state.obsSceneData,
    getTestMode: (state) => state.testMode,
    getVoices: (state) => state.voices,
    getMacros: (state) => state.macros,
    getAutoMacros: (state) => state.autoMacros,
    getVariables: (state) => state.variables,
    getGiveaway: (state) => state.giveaway,
    getYoloboxData: (state) => state.yoloboxData,
    getObsAudioData: (state) => state.obsAudioData,
    getAssets: (state) => state.assets,
    getStatus: (state) => state.status
  },
  actions: {
    async fetchConfig() {
      const request = await fetch(`/config.json`, { cache: "no-store" })
      const config = await request.json()

      this.$patch(state => state.config = {
        websocketPort: config.websocket.port,
        webserverPort: config.webserver.port,
      })
    },
    async fetchGames() {
      const request = await fetch(`${this.getRestApi}/api/games/all`, { cache: "no-store" })
      const data = (await request.json()).data

      this.games = data

      this.$patch(state => state.games = data)
    },
    setAlerts(alerts: []) {
      this.alerts = alerts
      this.$patch(state => state.alerts = alerts)
    },
    setWebsocketConnected(connected: boolean) {
      this.websocket.connected = connected
      this.$patch(state => state.websocket.connected = connected)
    },
    setWebsocketConnecting(connecting: boolean) {
      this.websocket.connecting = connecting
      this.$patch(state => state.websocket.connecting = connecting)
    },
    setCurrentGame(currentGame: any) {
      this.currentGame = currentGame
      this.$patch(state => state.currentGame = currentGame)
    },
    setShieldActive(shieldMode: boolean) {
      this.shieldMode = shieldMode
      this.$patch(state => state.shieldMode = shieldMode)
    },
    setChannelPoints(channelPoints: []) {
      this.channelPoints = channelPoints
      this.$patch(state => state.channelPoints = channelPoints)
    },
    setAudioData(audioData: {}) {
      this.audioData = audioData
      this.$patch(state => state.audioData = audioData)
    },
    setThrottled(throttled: boolean) {
      this.throttled = throttled
      this.$patch(state => state.throttled = throttled)
    },
    setSystemInfo(systemInfo: any) {
      this.systemInfo = systemInfo
      this.$patch(state => state.systemInfo = systemInfo)
    },
    setScene(scene: {}) {
      this.scene = scene
      this.$patch(state => state.scene = scene)
    },
    setConnections(connections: {}) {
      this.connections = connections
      this.$patch(state => state.connections = connections)
    },
    setBackendConfig(config: string, parsedConfig: any) {
      this.backendConfig = config
      this.parsedBackendConfig = parsedConfig
      this.$patch(state => state.backendConfig = config)
      this.$patch(state => state.parsedBackendConfig = parsedConfig)
    },
    setObsSceneData(obsSceneData: []) {
      this.obsSceneData = obsSceneData
      this.$patch(state => state.obsSceneData = obsSceneData)
    },
    setTestMode(testMode: boolean) {
      this.testMode = testMode
      this.$patch(state => state.testMode = testMode)
    },
    setVoices(voices: []) {
      this.voices = voices
      this.$patch(state => state.voices = voices)
    },
    setMacros(macros: {}) {
      this.macros = macros
      this.$patch(state => state.macros = macros)
    },
    setAutoMacros(autoMacros: []) {
      this.autoMacros = autoMacros
      this.$patch(state => state.autoMacros = autoMacros)
    },
    setVariables(variables: {}) {
      this.variables = variables
      this.$patch(state => state.variables = variables)
    },
    setGiveaway(giveaway: {}) {
      this.giveaway = giveaway
      this.$patch(state => state.giveaway = giveaway)
    },
    setYoloboxData(yoloboxData: {}) {
      this.yoloboxData = yoloboxData
      this.$patch(state => state.yoloboxData = yoloboxData)
    },
    setObsAudioData(obsAudioData: {}) {
      this.obsAudioData = obsAudioData
      this.$patch(state => state.obsAudioData = obsAudioData)
    },
    setAssets(assets: []) {
      this.assets = assets
      this.$patch(state => state.assets = assets)
    },
    setStatus(status: any) {
      this.status = status
      this.$patch(state => state.status = status)
    },
    async fetchStatus(): Promise<any> {
      let status = 'Unknown'
      try {
        status = (await (await fetch(`${this.getRestApi}/api/status`, { cache: "no-store" })).json()).data
      } catch (error) {
        console.warn(error)
      }

      this.setStatus(status)
      return status
    }
  }
})
