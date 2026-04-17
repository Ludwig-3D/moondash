import { createI18n } from 'vue-i18n'
import en from '@/locales/en.json'
import de from '@/locales/de.json'

const messages = {
    en,
    de,
}

export function normalizeLocale(input: string | null | undefined): string {
    if (!input) return 'en'

    const lower = input.toLowerCase()

    if (lower.startsWith('de')) return 'de'
    if (lower.startsWith('en')) return 'en'

    return 'en'
}

export function resolveLocale(configLanguage: string | null): string {
    if (configLanguage && configLanguage.trim()) {
        return normalizeLocale(configLanguage)
    }

    return normalizeLocale(navigator.language)
}

export const i18n = createI18n({
    legacy: false,
    locale: 'en',
    fallbackLocale: 'en',
    messages,
})