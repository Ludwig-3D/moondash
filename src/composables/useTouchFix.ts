import { onMounted, onUnmounted, ref } from 'vue'

export function useTouchFix() {
    const prevKey = ref('')

    const callback = (evt: TouchEvent) => {
        if (evt.touches.length <= 1) return

        const touch = evt.touches[evt.touches.length - 2]
        const key = `${touch.identifier}:${touch.clientX}:${touch.clientY}`

        if (key === prevKey.value) {
            window.location.reload()
            return
        }

        prevKey.value = key
    }

    onMounted(() => {
        document.addEventListener('touchstart', callback, { passive: true })
    })

    onUnmounted(() => {
        document.removeEventListener('touchstart', callback)
    })
}