import { useCallback, useEffect, useState } from 'react'
import {
  appearanceOptionFromPreferences,
  applyAppearanceOption,
  applyAppearancePreferences,
  loadAppearanceStyle,
  loadThemePreference,
  preferencesFromAppearanceOption,
  resolveTheme,
  saveAppearanceStyle,
  saveThemePreference,
  type AppearanceOption,
  type AppearanceStyle,
  type ResolvedTheme,
  type ThemePreference,
} from '../theme'

export const useTheme = () => {
  const [theme, setThemePreference] = useState<ThemePreference>(() =>
    loadThemePreference(),
  )
  const [style, setStylePreference] = useState<AppearanceStyle>(() =>
    loadAppearanceStyle(),
  )
  const [resolved, setResolved] = useState<ResolvedTheme>(() =>
    resolveTheme(loadThemePreference()),
  )
  const [appearance, setAppearanceOption] = useState<AppearanceOption>(() =>
    appearanceOptionFromPreferences(loadThemePreference(), loadAppearanceStyle()),
  )

  useEffect(() => {
    setResolved(applyAppearancePreferences(theme, style))
    setAppearanceOption(appearanceOptionFromPreferences(theme, style))
  }, [theme, style])

  useEffect(() => {
    if (theme !== 'system' || style === 'tactical') return

    const media = window.matchMedia('(prefers-color-scheme: dark)')
    const onChange = () => {
      setResolved(applyAppearancePreferences('system', 'default'))
      setAppearanceOption('system')
    }

    media.addEventListener('change', onChange)
    return () => media.removeEventListener('change', onChange)
  }, [theme, style])

  const setAppearance = useCallback((next: AppearanceOption) => {
    const { theme: nextTheme, style: nextStyle } =
      preferencesFromAppearanceOption(next)
    saveThemePreference(nextTheme)
    saveAppearanceStyle(nextStyle)
    setThemePreference(nextTheme)
    setStylePreference(nextStyle)
    setResolved(applyAppearanceOption(next))
    setAppearanceOption(next)
  }, [])

  return { theme, style, resolved, appearance, setAppearance }
}
