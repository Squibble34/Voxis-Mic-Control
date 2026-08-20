export type SystemSettings = {
  popupEnabled: boolean
  popupLocation: string
  soundEnabled: boolean
  soundVolume: number
  autoMuteEnabled: boolean
  autoMuteMinutes: number
}

export const DEFAULT_SYSTEM_SETTINGS: SystemSettings = {
  popupEnabled: true,
  popupLocation: "top-right",
  soundEnabled: true,
  soundVolume: 70,
  autoMuteEnabled: true,
  autoMuteMinutes: 5,
}

export type AppearanceSettings = {
  theme: "dark" | "light"
  // null means use the theme's built in default instead of a custom accent
  accentColor: string | null
  reduceMotion: boolean
}

export const DEFAULT_APPEARANCE_SETTINGS: AppearanceSettings = {
  theme: "dark",
  accentColor: null,
  reduceMotion: false,
}

// the actual computed --primary for each theme, used wherever we show or apply the "auto" accent
export const THEME_PRIMARY_HEX: Record<AppearanceSettings["theme"], string> = {
  dark: "#28d6df",
  light: "#00919d",
}

// simple luminance check to pick readable foreground text for an arbitrary accent color
export function getContrastForeground(hex: string): string {
  const clean = hex.replace("#", "")
  const r = parseInt(clean.substring(0, 2), 16) / 255
  const g = parseInt(clean.substring(2, 4), 16) / 255
  const b = parseInt(clean.substring(4, 6), 16) / 255
  const luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b
  return luminance > 0.6 ? "oklch(0.145 0 0)" : "oklch(0.985 0 0)"
}

export type KeybindsSettings = {
  hotkeysEnabled: boolean
  hotkeyMode: "toggle" | "push-to-talk"
  muteChord: string[]
  openAppChord: string[]
}

export const DEFAULT_KEYBINDS_SETTINGS: KeybindsSettings = {
  hotkeysEnabled: true,
  hotkeyMode: "toggle",
  muteChord: ["Ctrl", "Alt", "M"],
  openAppChord: ["Ctrl", "Alt", "A"],
}

export type WindowsSettings = {
  launchAtStartup: boolean
  startupState: "tray" | "visible" | "minimized"
  startupDelay: number
  minimizeToTrayOnClose: boolean
  rememberWindowPosition: boolean
  alwaysOnTop: boolean
}

export const DEFAULT_WINDOWS_SETTINGS: WindowsSettings = {
  launchAtStartup: true,
  startupState: "minimized",
  startupDelay: 3,
  minimizeToTrayOnClose: true,
  rememberWindowPosition: true,
  alwaysOnTop: false,
}