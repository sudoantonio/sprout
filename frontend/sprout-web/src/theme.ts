export type ThemePreference = 'light' | 'dark' | 'system'

export type ResolvedTheme = 'light' | 'dark'

export type AppearanceStyle = 'default' | 'tactical'

/** UI-facing appearance choice shown in Appearance settings */
export type AppearanceOption =
  | 'light'
  | 'dark'
  | 'system'
  | 'tactical-light'
  | 'tactical-shadow'

export const THEME_STORAGE_KEY = 'sprout-theme'
export const APPEARANCE_STYLE_STORAGE_KEY = 'sprout-appearance-style'

const isThemePreference = (value: string): value is ThemePreference =>
  value === 'light' || value === 'dark' || value === 'system'

const isAppearanceStyle = (value: string): value is AppearanceStyle =>
  value === 'default' || value === 'tactical'

export const loadThemePreference = (): ThemePreference => {
  try {
    const stored = localStorage.getItem(THEME_STORAGE_KEY)
    if (stored && isThemePreference(stored)) {
      return stored
    }
  } catch {
    // Ignore storage errors (private mode, blocked storage, etc.)
  }
  return 'system'
}

export const loadAppearanceStyle = (): AppearanceStyle => {
  try {
    const stored = localStorage.getItem(APPEARANCE_STYLE_STORAGE_KEY)
    if (stored && isAppearanceStyle(stored)) {
      return stored
    }
  } catch {
    // Ignore storage errors
  }
  return 'default'
}

export const saveThemePreference = (preference: ThemePreference): void => {
  try {
    localStorage.setItem(THEME_STORAGE_KEY, preference)
  } catch {
    // Ignore storage errors
  }
}

export const saveAppearanceStyle = (style: AppearanceStyle): void => {
  try {
    localStorage.setItem(APPEARANCE_STYLE_STORAGE_KEY, style)
  } catch {
    // Ignore storage errors
  }
}

export const resolveTheme = (preference: ThemePreference): ResolvedTheme => {
  if (preference === 'light' || preference === 'dark') {
    return preference
  }
  if (typeof window !== 'undefined' && window.matchMedia) {
    return window.matchMedia('(prefers-color-scheme: dark)').matches
      ? 'dark'
      : 'light'
  }
  return 'light'
}

export const appearanceOptionFromPreferences = (
  theme: ThemePreference,
  style: AppearanceStyle,
): AppearanceOption => {
  if (style === 'tactical') {
    return resolveTheme(theme) === 'dark' ? 'tactical-shadow' : 'tactical-light'
  }
  return theme
}

export const preferencesFromAppearanceOption = (
  option: AppearanceOption,
): { theme: ThemePreference; style: AppearanceStyle } => {
  switch (option) {
    case 'tactical-light':
      return { theme: 'light', style: 'tactical' }
    case 'tactical-shadow':
      return { theme: 'dark', style: 'tactical' }
    case 'light':
    case 'dark':
    case 'system':
      return { theme: option, style: 'default' }
  }
}

export const applyResolvedTheme = (resolved: ResolvedTheme): void => {
  document.documentElement.dataset.theme = resolved
}

export const applyAppearanceStyle = (style: AppearanceStyle): void => {
  document.documentElement.dataset.style = style
}

export const applyThemePreference = (preference: ThemePreference): ResolvedTheme => {
  const resolved = resolveTheme(preference)
  applyResolvedTheme(resolved)
  return resolved
}

export const applyAppearancePreferences = (
  theme: ThemePreference,
  style: AppearanceStyle,
): ResolvedTheme => {
  applyAppearanceStyle(style)
  return applyThemePreference(theme)
}

export const applyAppearanceOption = (option: AppearanceOption): ResolvedTheme => {
  const { theme, style } = preferencesFromAppearanceOption(option)
  return applyAppearancePreferences(theme, style)
}

export const initThemeFromStorage = (): ResolvedTheme => {
  const theme = loadThemePreference()
  const style = loadAppearanceStyle()
  return applyAppearancePreferences(theme, style)
}