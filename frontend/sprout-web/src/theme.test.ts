import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import {
  APPEARANCE_STYLE_STORAGE_KEY,
  THEME_STORAGE_KEY,
  applyAppearanceOption,
  applyAppearancePreferences,
  applyThemePreference,
  appearanceOptionFromPreferences,
  loadAppearanceStyle,
  loadThemePreference,
  preferencesFromAppearanceOption,
  resolveTheme,
  saveAppearanceStyle,
  saveThemePreference,
} from './theme'

describe('theme', () => {
  beforeEach(() => {
    localStorage.clear()
    document.documentElement.removeAttribute('data-theme')
    document.documentElement.removeAttribute('data-style')
  })

  afterEach(() => {
    localStorage.clear()
    document.documentElement.removeAttribute('data-theme')
    document.documentElement.removeAttribute('data-style')
  })

  it('defaults to system when nothing is stored', () => {
    expect(loadThemePreference()).toBe('system')
  })

  it('defaults appearance style to default when nothing is stored', () => {
    expect(loadAppearanceStyle()).toBe('default')
  })

  it('persists and loads theme preference', () => {
    saveThemePreference('dark')
    expect(loadThemePreference()).toBe('dark')
    saveThemePreference('light')
    expect(loadThemePreference()).toBe('light')
  })

  it('persists and loads appearance style', () => {
    saveAppearanceStyle('tactical')
    expect(loadAppearanceStyle()).toBe('tactical')
    saveAppearanceStyle('default')
    expect(loadAppearanceStyle()).toBe('default')
  })

  it('ignores invalid stored values', () => {
    localStorage.setItem(THEME_STORAGE_KEY, 'invalid')
    expect(loadThemePreference()).toBe('system')
    localStorage.setItem(APPEARANCE_STYLE_STORAGE_KEY, 'invalid')
    expect(loadAppearanceStyle()).toBe('default')
  })

  it('resolves explicit light and dark preferences', () => {
    expect(resolveTheme('light')).toBe('light')
    expect(resolveTheme('dark')).toBe('dark')
  })

  it('applies resolved theme to the document root', () => {
    applyThemePreference('dark')
    expect(document.documentElement.dataset.theme).toBe('dark')
    applyThemePreference('light')
    expect(document.documentElement.dataset.theme).toBe('light')
  })

  it('applies appearance style to the document root', () => {
    applyAppearancePreferences('light', 'tactical')
    expect(document.documentElement.dataset.theme).toBe('light')
    expect(document.documentElement.dataset.style).toBe('tactical')
    applyAppearancePreferences('dark', 'default')
    expect(document.documentElement.dataset.theme).toBe('dark')
    expect(document.documentElement.dataset.style).toBe('default')
  })

  it('maps appearance options to stored preferences', () => {
    expect(preferencesFromAppearanceOption('tactical-light')).toEqual({
      theme: 'light',
      style: 'tactical',
    })
    expect(preferencesFromAppearanceOption('tactical-shadow')).toEqual({
      theme: 'dark',
      style: 'tactical',
    })
    expect(preferencesFromAppearanceOption('system')).toEqual({
      theme: 'system',
      style: 'default',
    })
  })

  it('derives appearance options from stored preferences', () => {
    expect(appearanceOptionFromPreferences('light', 'default')).toBe('light')
    expect(appearanceOptionFromPreferences('dark', 'tactical')).toBe(
      'tactical-shadow',
    )
    expect(appearanceOptionFromPreferences('light', 'tactical')).toBe(
      'tactical-light',
    )
  })

  it('applies tactical-light and tactical-shadow options', () => {
    applyAppearanceOption('tactical-light')
    expect(document.documentElement.dataset.theme).toBe('light')
    expect(document.documentElement.dataset.style).toBe('tactical')

    applyAppearanceOption('tactical-shadow')
    expect(document.documentElement.dataset.theme).toBe('dark')
    expect(document.documentElement.dataset.style).toBe('tactical')
  })
})
