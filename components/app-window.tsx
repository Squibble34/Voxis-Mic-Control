"use client"

import * as React from "react"
import { invoke } from "@tauri-apps/api/core"
import { emit, listen } from "@tauri-apps/api/event"
import { load, type Store } from "@tauri-apps/plugin-store"

import { AppTrackerCard, type TrackedApp } from "@/components/app-tracker-card"
import { HotkeyCard } from "@/components/hotkey-card"
import { MicController, type Device } from "@/components/mic-controller"
import { cn } from "@/lib/utils"
import { VuMeterCard } from "@/components/vu-meter-card"
import { SettingsPanel } from "@/components/settings-panel"
import { StatusBanner } from "@/components/status-banner"
import { StatusBar } from "@/components/status-bar"
import { TitleBar, type AppView } from "@/components/title-bar"
import { UpdateBanner } from "@/components/update-banner"
import { getCurrentWindow } from "@tauri-apps/api/window"
import { getVersion } from "@tauri-apps/api/app"
import {
  disable as disableAutostart,
  enable as enableAutostart,
  isEnabled as isAutostartEnabled,
} from "@tauri-apps/plugin-autostart"
import { MIC_STATUS, type MicStatus } from "@/lib/mic-status"
import {
  DEFAULT_APPEARANCE_SETTINGS,
  DEFAULT_KEYBINDS_SETTINGS,
  DEFAULT_SYSTEM_SETTINGS,
  DEFAULT_WINDOWS_SETTINGS,
  getContrastForeground,
  THEME_PRIMARY_HEX,
  type AppearanceSettings,
  type KeybindsSettings,
  type SystemSettings,
  type WindowsSettings,
} from "@/lib/settings-types"
import { useGlobalHotkey } from "@/lib/use-global-hotkey"
import { useWindowSizeSync } from "@/lib/use-window-size-sync"

const WINDOW_WIDTH = 644
// same height for the whole settings view no matter which sidebar section is open
const SETTINGS_HEIGHT = 552

function hslToHex(h: number, s: number, l: number): string {
  const sat = s / 100
  const light = l / 100
  const k = (n: number) => (n + h / 30) % 12
  const a = sat * Math.min(light, 1 - light)
  const f = (n: number) =>
    light - a * Math.max(-1, Math.min(k(n) - 3, Math.min(9 - k(n), 1)))
  const toHex = (n: number) => Math.round(255 * f(n)).toString(16).padStart(2, "0")
  return `#${toHex(0)}${toHex(8)}${toHex(4)}`
}

function playMuteTone(nowMuted: boolean, volume: number) {
  const peak = 0.15 * Math.max(0, Math.min(1, volume / 100))
  if (peak <= 0.0001) return // fully silent, skip creating a context at all

  const AudioCtx = window.AudioContext || (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext
  const ctx = new AudioCtx()
  const oscillator = ctx.createOscillator()
  const gain = ctx.createGain()
  oscillator.type = "sine"
  oscillator.frequency.value = nowMuted ? 320 : 480
  gain.gain.setValueAtTime(peak, ctx.currentTime)
  gain.gain.exponentialRampToValueAtTime(0.0001, ctx.currentTime + 0.12)
  oscillator.connect(gain)
  gain.connect(ctx.destination)
  oscillator.start()
  oscillator.stop(ctx.currentTime + 0.12)
  oscillator.onended = () => ctx.close()
}

export function AppWindow() {
  const [activeView, setActiveView] = React.useState<AppView>("main")
  const [status, setStatus] = React.useState<MicStatus>("ready")
  const [dismissed, setDismissed] = React.useState(false)
  const [busy, setBusy] = React.useState(false)
  const [devices, setDevices] = React.useState<Device[]>([])
  const [deviceId, setDeviceId] = React.useState("")
  const [muted, setMuted] = React.useState(false)
  const [volume, setVolume] = React.useState(0)
  const [muteError, setMuteError] = React.useState<string | null>(null)

  // system settings, persisted via the settings store
  const [popupEnabled, setPopupEnabled] = React.useState(DEFAULT_SYSTEM_SETTINGS.popupEnabled)
  const [popupLocation, setPopupLocation] = React.useState(DEFAULT_SYSTEM_SETTINGS.popupLocation)
  const [soundEnabled, setSoundEnabled] = React.useState(DEFAULT_SYSTEM_SETTINGS.soundEnabled)
  const [soundVolume, setSoundVolume] = React.useState(DEFAULT_SYSTEM_SETTINGS.soundVolume)
  const [autoMuteEnabled, setAutoMuteEnabled] = React.useState(DEFAULT_SYSTEM_SETTINGS.autoMuteEnabled)
  const [autoMuteMinutes, setAutoMuteMinutes] = React.useState(DEFAULT_SYSTEM_SETTINGS.autoMuteMinutes)

  // refs so the listener below, registered once, always reads the latest values
  const soundEnabledRef = React.useRef(soundEnabled)
  const soundVolumeRef = React.useRef(soundVolume)
  React.useEffect(() => {
    soundEnabledRef.current = soundEnabled
  }, [soundEnabled])
  React.useEffect(() => {
    soundVolumeRef.current = soundVolume
  }, [soundVolume])

  function updateSystemSettings(patch: Partial<SystemSettings>) {
    if (patch.popupEnabled !== undefined) setPopupEnabled(patch.popupEnabled)
    if (patch.popupLocation !== undefined) setPopupLocation(patch.popupLocation)
    if (patch.soundEnabled !== undefined) setSoundEnabled(patch.soundEnabled)
    if (patch.soundVolume !== undefined) setSoundVolume(patch.soundVolume)
    if (patch.autoMuteEnabled !== undefined) setAutoMuteEnabled(patch.autoMuteEnabled)
    if (patch.autoMuteMinutes !== undefined) setAutoMuteMinutes(patch.autoMuteMinutes)
  }

  // appearance settings, persisted via the settings store
  const [theme, setTheme] = React.useState(DEFAULT_APPEARANCE_SETTINGS.theme)
  const [accentColor, setAccentColor] = React.useState(DEFAULT_APPEARANCE_SETTINGS.accentColor)
  const [reduceMotion, setReduceMotion] = React.useState(DEFAULT_APPEARANCE_SETTINGS.reduceMotion)

  function updateAppearanceSettings(patch: Partial<AppearanceSettings>) {
    if (patch.theme !== undefined) setTheme(patch.theme)
    if (patch.accentColor !== undefined) setAccentColor(patch.accentColor)
    if (patch.reduceMotion !== undefined) setReduceMotion(patch.reduceMotion)
  }

  React.useEffect(() => {
    const root = document.documentElement
    root.classList.toggle("dark", theme === "dark")
    root.classList.toggle("light", theme === "light")
  }, [theme])

  React.useEffect(() => {
    const root = document.documentElement
    if (accentColor) {
      // inline style on <html> beats the .dark/.light class rules, so a custom accent always wins over the theme
      root.style.setProperty("--primary", accentColor)
      root.style.setProperty("--primary-foreground", getContrastForeground(accentColor))
    } else {
      root.style.removeProperty("--primary")
      root.style.removeProperty("--primary-foreground")
    }
  }, [accentColor])

  React.useEffect(() => {
    document.documentElement.classList.toggle("reduce-motion", reduceMotion)
  }, [reduceMotion])

  // keybinds settings, persisted via the settings store
  const [hotkeysEnabled, setHotkeysEnabled] = React.useState(DEFAULT_KEYBINDS_SETTINGS.hotkeysEnabled)
  const [hotkeyMode, setHotkeyMode] = React.useState(DEFAULT_KEYBINDS_SETTINGS.hotkeyMode)
  const [muteChord, setMuteChord] = React.useState(DEFAULT_KEYBINDS_SETTINGS.muteChord)
  const [openAppChord, setOpenAppChord] = React.useState(DEFAULT_KEYBINDS_SETTINGS.openAppChord)

  function updateKeybindsSettings(patch: Partial<KeybindsSettings>) {
    if (patch.hotkeysEnabled !== undefined) setHotkeysEnabled(patch.hotkeysEnabled)
    if (patch.hotkeyMode !== undefined) setHotkeyMode(patch.hotkeyMode)
    if (patch.muteChord !== undefined) setMuteChord(patch.muteChord)
    if (patch.openAppChord !== undefined) setOpenAppChord(patch.openAppChord)
  }

  // windows settings, persisted via the settings store
  const [launchAtStartup, setLaunchAtStartup] = React.useState(DEFAULT_WINDOWS_SETTINGS.launchAtStartup)
  const [startupState, setStartupState] = React.useState(DEFAULT_WINDOWS_SETTINGS.startupState)
  const [startupDelay, setStartupDelay] = React.useState(DEFAULT_WINDOWS_SETTINGS.startupDelay)
  const [minimizeToTrayOnClose, setMinimizeToTrayOnClose] = React.useState(
    DEFAULT_WINDOWS_SETTINGS.minimizeToTrayOnClose,
  )

  const [rememberWindowPosition, setRememberWindowPosition] = React.useState(
    DEFAULT_WINDOWS_SETTINGS.rememberWindowPosition,
  )
  const [alwaysOnTop, setAlwaysOnTop] = React.useState(DEFAULT_WINDOWS_SETTINGS.alwaysOnTop)

  function updateWindowsSettings(patch: Partial<WindowsSettings>) {
    if (patch.launchAtStartup !== undefined) setLaunchAtStartup(patch.launchAtStartup)
    if (patch.startupState !== undefined) setStartupState(patch.startupState)
    if (patch.startupDelay !== undefined) setStartupDelay(patch.startupDelay)
      if (patch.minimizeToTrayOnClose !== undefined) setMinimizeToTrayOnClose(patch.minimizeToTrayOnClose)
        if (patch.rememberWindowPosition !== undefined) setRememberWindowPosition(patch.rememberWindowPosition)
        if (patch.alwaysOnTop !== undefined) setAlwaysOnTop(patch.alwaysOnTop)
      }

  React.useEffect(() => {
    const unlistenPromise = listen("auto-muted", () => {
      setMuted(true)
      if (soundEnabledRef.current) playMuteTone(true, soundVolumeRef.current)
      emit("mute-changed", { muted: true }).catch(console.error)
    })
    return () => {
      unlistenPromise.then((unlisten) => unlisten())
    }
  }, [])

  const [pendingUpdate, setPendingUpdate] = React.useState<{ version: string; notes: string } | null>(null)
  const [currentVersion, setCurrentVersion] = React.useState<string>("")

  React.useEffect(() => {
    getVersion().then(setCurrentVersion).catch(() => {})
  }, [])

  React.useEffect(() => {
    const unlistenPromise = listen<{ version: string; notes: string }>("update-available", (event) => {
      setPendingUpdate(event.payload)
    })
    return () => {
      unlistenPromise.then((unlisten) => unlisten())
    }
  }, [])

  const [inputLevel, setInputLevel] = React.useState<number | undefined>(undefined)

  React.useEffect(() => {
    const unlistenPromise = listen<number>("input-level-changed", (event) => {
      setInputLevel(event.payload)
    })
    return () => {
      unlistenPromise.then((unlisten) => unlisten())
    }
  }, [])

  const [trackedApps, setTrackedApps] = React.useState<TrackedApp[]>([])

  React.useEffect(() => {
    if (!deviceId) {
      setTrackedApps([])
      return
    }
    let cancelled = false
    const poll = () => {
      invoke<TrackedApp[]>("get_capturing_sessions", { deviceId })
        .then((apps) => {
          if (!cancelled) setTrackedApps(apps)
        })
        .catch(console.error)
    }
    poll()
    const interval = window.setInterval(poll, 1500)
    return () => {
      cancelled = true
      window.clearInterval(interval)
    }
  }, [deviceId])

  // debounced separately from the ui update below so dragging the slider doesnt flood windows with calls
  const volumeDebounce = React.useRef<ReturnType<typeof setTimeout> | null>(null)
  const storeRef = React.useRef<Store | null>(null)
  const savedDeviceIdRef = React.useRef<string | null>(null)
  const [settingsReady, setSettingsReady] = React.useState(false)
  // tracks the last time the user changed mute or volume so the background sync below doesnt fight a local change
  const lastLocalChange = React.useRef(0)

  const syncDeviceState = React.useCallback((id: string) => {
    if (!id) return
    invoke<boolean>("get_mute_state", { deviceId: id })
      .then(setMuted)
      .catch(console.error)
    invoke<number>("get_current_volume", { deviceId: id })
      .then((level) => setVolume(Math.round(level * 100)))
      .catch(console.error)
  }, [])

  // loads persisted settings once, before device polling starts
  React.useEffect(() => {
    load("settings.json", { autoSave: false })
      .then((store) => {
        storeRef.current = store
        return Promise.all([
          store.get<string>("lastDeviceId"),
          store.get<SystemSettings>("systemSettings"),
          store.get<AppearanceSettings>("appearanceSettings"),
          store.get<KeybindsSettings>("keybindsSettings"),
          store.get<WindowsSettings>("windowsSettings"),
        ])
      })
      .then(([savedDeviceId, savedSystemSettings, savedAppearanceSettings, savedKeybindsSettings, savedWindowsSettings]) => {
        savedDeviceIdRef.current = savedDeviceId ?? null

        const system = { ...DEFAULT_SYSTEM_SETTINGS, ...savedSystemSettings }
        setPopupEnabled(system.popupEnabled)
        setPopupLocation(system.popupLocation)
        setSoundEnabled(system.soundEnabled)
        setSoundVolume(system.soundVolume)
        setAutoMuteEnabled(system.autoMuteEnabled)
        setAutoMuteMinutes(system.autoMuteMinutes)

        const appearance = { ...DEFAULT_APPEARANCE_SETTINGS, ...savedAppearanceSettings }
        setTheme(appearance.theme)
        setAccentColor(appearance.accentColor)
        setReduceMotion(appearance.reduceMotion)

        const keybinds = { ...DEFAULT_KEYBINDS_SETTINGS, ...savedKeybindsSettings }
        setHotkeysEnabled(keybinds.hotkeysEnabled)
        setHotkeyMode(keybinds.hotkeyMode)
        setMuteChord(keybinds.muteChord)
        setOpenAppChord(keybinds.openAppChord)

        const windows = { ...DEFAULT_WINDOWS_SETTINGS, ...savedWindowsSettings }
        setLaunchAtStartup(windows.launchAtStartup)
        setStartupState(windows.startupState)
        setStartupDelay(windows.startupDelay)
        setMinimizeToTrayOnClose(windows.minimizeToTrayOnClose)
        setRememberWindowPosition(windows.rememberWindowPosition)
        setAlwaysOnTop(windows.alwaysOnTop)
      })
      .catch((err) => console.error("Failed to load settings:", err))
      .finally(() => setSettingsReady(true))
  }, [])

  // persists system settings and pushes auto mute config to the rust watcher whenever any of these change, including right after the load above
  React.useEffect(() => {
    if (!settingsReady) return
    const system: SystemSettings = {
      popupEnabled,
      popupLocation,
      soundEnabled,
      soundVolume,
      autoMuteEnabled,
      autoMuteMinutes,
    }
    storeRef.current?.set("systemSettings", system)
    storeRef.current?.save().catch(console.error)
    invoke("set_auto_mute_enabled", { enabled: autoMuteEnabled }).catch(console.error)
    invoke("set_auto_mute_idle_minutes", { minutes: autoMuteMinutes }).catch(console.error)
  }, [settingsReady, popupEnabled, popupLocation, soundEnabled, soundVolume, autoMuteEnabled, autoMuteMinutes])

  React.useEffect(() => {
    if (!settingsReady) return
    const appearance: AppearanceSettings = { theme, accentColor, reduceMotion }
    storeRef.current?.set("appearanceSettings", appearance)
    storeRef.current?.save().catch(console.error)
  }, [settingsReady, theme, accentColor, reduceMotion])

  React.useEffect(() => {
    if (!settingsReady) return
    const keybinds: KeybindsSettings = { hotkeysEnabled, hotkeyMode, muteChord, openAppChord }
    storeRef.current?.set("keybindsSettings", keybinds)
    storeRef.current?.save().catch(console.error)
  }, [settingsReady, hotkeysEnabled, hotkeyMode, muteChord, openAppChord])

  React.useEffect(() => {
    if (!settingsReady) return
    const windows: WindowsSettings = {
      launchAtStartup,
      startupState,
      startupDelay,
      minimizeToTrayOnClose,
      rememberWindowPosition,
      alwaysOnTop,
    }
    storeRef.current?.set("windowsSettings", windows)
    storeRef.current?.save().catch(console.error)
  }, [
    settingsReady,
    launchAtStartup,
    startupState,
    startupDelay,
    minimizeToTrayOnClose,
    rememberWindowPosition,
    alwaysOnTop,
  ])

  // debounced, and only runs while remember window position is enabled
  React.useEffect(() => {
    if (!settingsReady || !rememberWindowPosition) return

    let debounceId: ReturnType<typeof setTimeout> | null = null

    const unlistenPromise = getCurrentWindow().onMoved((position) => {
      if (debounceId) clearTimeout(debounceId)
      debounceId = setTimeout(() => {
        storeRef.current?.set("windowPosition", { x: position.x, y: position.y })
        storeRef.current?.save().catch(console.error)
      }, 500)
    })

    return () => {
      if (debounceId) clearTimeout(debounceId)
      unlistenPromise.then((unlisten) => unlisten())
    }
  }, [settingsReady, rememberWindowPosition])

  // isolated from the persist effect above, this is the only thing that should ever touch the os level
  // autostart registration, and only when the toggle it corresponds to actually changes. checks the real
  // os state first so it doesnt call disable() on something that isnt cleanly registered, which is what
  // was throwing in dev mode
  React.useEffect(() => {
    if (!settingsReady) return
    ;(async () => {
      try {
        const currentlyEnabled = await isAutostartEnabled()
        if (launchAtStartup && !currentlyEnabled) {
          await enableAutostart()
        } else if (!launchAtStartup && currentlyEnabled) {
          await disableAutostart()
        }
      } catch {
        // dev mode autostart registration is flaky since the debug binarys path changes on every rebuild
        // harmless, a production build wont hit this
      }
    })()
  }, [settingsReady, launchAtStartup])

  React.useEffect(() => {
    if (!settingsReady) return
    invoke("set_minimize_to_tray_on_close", { enabled: minimizeToTrayOnClose }).catch(console.error)
  }, [settingsReady, minimizeToTrayOnClose])

  React.useEffect(() => {
    if (!settingsReady) return
    getCurrentWindow().setAlwaysOnTop(alwaysOnTop).catch(console.error)
  }, [settingsReady, alwaysOnTop])

  React.useEffect(() => {
    invoke("set_tray_icon", { muted, accentColor: accentColor ?? THEME_PRIMARY_HEX[theme] }).catch(console.error)
  }, [muted, accentColor])

  // the popup is a separate window and cant see this components state, so tell it about popup relevant changes over an event instead
  React.useEffect(() => {
    if (!settingsReady) return
    emit("system-settings-changed", { popupEnabled, popupLocation }).catch(console.error)
  }, [settingsReady, popupEnabled, popupLocation])

  React.useEffect(() => {
    if (!settingsReady) return
    emit("appearance-settings-changed", { theme, accentColor, reduceMotion }).catch(console.error)
  }, [settingsReady, theme, accentColor, reduceMotion])

  const refreshDevices = React.useCallback(() => {
    invoke<Device[]>("get_audio_devices")
      .then((devs) => {
        setDevices(devs)

        if (devs.length === 0) {
          setStatus("no-devices")
          return
        }

        setDeviceId((currentId) => {
          if (!currentId) {
            const preferred = savedDeviceIdRef.current
            const target =
              preferred && devs.some((d) => d.id === preferred) ? preferred : devs[0].id

            syncDeviceState(target)
            setStatus("ready")
            invoke("set_monitor_device", { deviceId: target }).catch(console.error)

            // mute on start, only runs on this first resolution and only if the device isnt already muted
            invoke<boolean>("get_mute_state", { deviceId: target })
              .then((isMuted) => {
                if (!isMuted) {
                  invoke("set_mute_state", { deviceId: target, muted: true }).catch(console.error)
                  setMuted(true)
                }
              })
              .catch(console.error)

            return target
          }

          const stillPresent = devs.some((d) => d.id === currentId)

          if (!stillPresent) {
            setStatus("disconnected")
            return currentId
          }

          setStatus((prevStatus) => {
            if (prevStatus === "disconnected" || prevStatus === "reconnecting") {
              window.setTimeout(() => setStatus("ready"), 900)
              return "reconnecting"
            }
            if (prevStatus === "no-devices") return "ready"
            return prevStatus
          })

          // syncs regardless of status, this is what catches mute/volume changes made outside the app like a hardware button or the windows mixer
          if (Date.now() - lastLocalChange.current > 2500) {
            syncDeviceState(currentId)
          }

          return currentId
        })
      })
      .catch(console.error)
  }, [syncDeviceState])

  // initial load then poll every 2s, windows doesnt push plug/unplug events to us here so we detect them by re-checking the device list
  React.useEffect(() => {
    if (!settingsReady) return
    refreshDevices()
    const interval = window.setInterval(refreshDevices, 2000)
    return () => window.clearInterval(interval)
  }, [refreshDevices, settingsReady])

  const handleMutedChange = React.useCallback(
    (newMuted: boolean) => {
      lastLocalChange.current = Date.now()
      const previous = muted
      setMuted(newMuted)
      setMuteError(null)

      invoke("set_mute_state", { deviceId, muted: newMuted })
      .then(() => invoke<boolean>("get_mute_state", { deviceId }))
      .then((actual) => {
        setMuted(actual)
        if (soundEnabledRef.current) playMuteTone(actual, soundVolumeRef.current)
        emit("mute-changed", { muted: actual }).catch(console.error)
      })
      .catch((err) => {
          console.error(err)
          setMuted(previous)
          setMuteError("Couldn't change mute state")
          window.setTimeout(() => setMuteError(null), 3000)
        })
    },
    [deviceId, muted],
  )

  const handleVolumeChange = React.useCallback(
    (newVolume: number) => {
      lastLocalChange.current = Date.now()
      setVolume(newVolume)
      if (volumeDebounce.current) clearTimeout(volumeDebounce.current)
      volumeDebounce.current = setTimeout(() => {
        invoke("set_volume", { deviceId, level: newVolume / 100.0 }).catch(console.error)
      }, 80)
    },
    [deviceId],
  )

  const descriptor = MIC_STATUS[status]

  // registered once here regardless of which view is showing, switching to settings or anywhere else never unregisters these
  const { registerError: muteRegisterError } = useGlobalHotkey(
    muteChord,
    hotkeysEnabled && !descriptor.controlsDisabled,
    () => {
      if (hotkeyMode === "push-to-talk") {
        handleMutedChange(false)
      } else {
        handleMutedChange(!muted)
      }
    },
    {
      mode: hotkeyMode,
      onRelease: () => handleMutedChange(true),
    },
  )
  useGlobalHotkey(openAppChord, hotkeysEnabled, () => {
    const win = getCurrentWindow()
    win.show().then(() => win.unminimize()).then(() => win.setFocus()).catch(console.error)
  })

  // switching hotkey mode resets to muted so push-to-talk always starts from its expected resting state.
  // skips the very first run after settings load so restoring a saved push-to-talk mode on launch doesnt
  // force an extra mute call
  const previousHotkeyModeRef = React.useRef(hotkeyMode)
  const hotkeyModeInitializedRef = React.useRef(false)
  React.useEffect(() => {
    if (!settingsReady) return
    if (!hotkeyModeInitializedRef.current) {
      hotkeyModeInitializedRef.current = true
      previousHotkeyModeRef.current = hotkeyMode
      return
    }
    if (previousHotkeyModeRef.current !== hotkeyMode) {
      previousHotkeyModeRef.current = hotkeyMode
      handleMutedChange(true)
    }
  }, [settingsReady, hotkeyMode, handleMutedChange])

  // fun mode (danger zone). intentionally not persisted, resets to off every launch, but lives here
  // rather than in SettingsPanel so it keeps running when you leave settings instead of unmounting
  // and snapping back to off
  const [funModeUnlocked, setFunModeUnlocked] = React.useState(false)
  const [funModeEnabled, setFunModeEnabled] = React.useState(false)
  const [crazyColors, setCrazyColors] = React.useState(false)
  const [randomMute, setRandomMute] = React.useState(false)
  const [normalVolume, setNormalVolume] = React.useState(false)

  const accentColorRef = React.useRef(accentColor)
  React.useEffect(() => {
    accentColorRef.current = accentColor
  }, [accentColor])

  // empty deps for a stable identity, otherwise the effect below gets torn down and restarted on every
  // unrelated render, that was the flashing bug
  const setAccentPreview = React.useCallback((hex: string | null) => {
    const root = document.documentElement
    const target = hex ?? accentColorRef.current
    if (target) {
      root.style.setProperty("--primary", target)
      root.style.setProperty("--primary-foreground", getContrastForeground(target))
    } else {
      root.style.removeProperty("--primary")
      root.style.removeProperty("--primary-foreground")
    }
  }, [])

  React.useEffect(() => {
    if (!crazyColors) {
      setAccentPreview(null)
      return
    }
    let hue = 0
    const interval = window.setInterval(() => {
      hue = (hue + 3) % 360
      setAccentPreview(hslToHex(hue, 85, 60))
    }, 40)
    return () => {
      window.clearInterval(interval)
      setAccentPreview(null)
    }
  }, [crazyColors, setAccentPreview])

  // refs so these two schedulers always call the current handler without needing it in a dependency array.
  // handleMutedChange in particular gets a new identity on every real mute toggle, which would otherwise
  // reset the random mute timer every time it actually fires
  const handleMutedChangeRef = React.useRef(handleMutedChange)
  React.useEffect(() => {
    handleMutedChangeRef.current = handleMutedChange
  })
  const handleVolumeChangeRef = React.useRef(handleVolumeChange)
  React.useEffect(() => {
    handleVolumeChangeRef.current = handleVolumeChange
  })

  React.useEffect(() => {
    if (!randomMute) return
    let timeoutId: ReturnType<typeof setTimeout> | number | null = null
    const scheduleNext = () => {
      const delay = 30_000 + Math.random() * 60_000
      timeoutId = window.setTimeout(() => {
        handleMutedChangeRef.current(true)
        scheduleNext()
      }, delay)
    }
    scheduleNext()
    return () => {
      if (timeoutId) window.clearTimeout(timeoutId as number)
    }
  }, [randomMute])

  React.useEffect(() => {
    if (!normalVolume) return
    let timeoutId: ReturnType<typeof setTimeout> | number | null = null
    const scheduleNext = () => {
      const delay = 3_000 + Math.random() * 7_000
      timeoutId = window.setTimeout(() => {
        handleVolumeChangeRef.current(Math.round(Math.random() * 100))
        scheduleNext()
      }, delay)
    }
    scheduleNext()
    return () => {
      if (timeoutId) window.clearTimeout(timeoutId as number)
    }
  }, [normalVolume])

  // a disconnected endpoint disappears from enumeration, no devices empties it
  const displayDevices = React.useMemo(() => {
    if (status === "no-devices") return []
    if (status === "disconnected" || status === "reconnecting") {
      return devices.map((device) =>
        device.id === deviceId ? { ...device, available: false } : device,
      )
    }
    return devices
  }, [status, deviceId, devices])

  function handleAction() {
    // dismiss only hides the advisory banner, the constraint itself remains
    if (status === "in-use" || status === "volume-unsupported") {
      setDismissed(true)
      return
    }

    if (status === "disconnected") {
      setBusy(true)
      refreshDevices()
      window.setTimeout(() => setBusy(false), 900)
    }
  }

  function handleDeviceChange(id: string) {
    setDeviceId(id)
    syncDeviceState(id)
    if (status === "disconnected" || status === "no-devices") setStatus("ready")

    storeRef.current?.set("lastDeviceId", id)
    storeRef.current?.save().catch(console.error)
    invoke("set_monitor_device", { deviceId: id }).catch(console.error)
  }

  const showBanner = Boolean(descriptor.title) && !dismissed

  const titleBarRef = React.useRef<HTMLDivElement>(null)
  const mainRef = React.useRef<HTMLDivElement>(null)
  const statusBarRef = React.useRef<HTMLDivElement>(null)

  useWindowSizeSync({
    titleBarRef,
    contentRef: mainRef,
    statusBarRef,
    width: WINDOW_WIDTH,
    settingsHeight: SETTINGS_HEIGHT,
    contentPaddingY: 24, // <main>'s p-3 adds 12px top and bottom that scrollHeight on the inner ref wont include
    reduceMotion,
    viewKey: activeView,
  })

  return (
    <div className="flex h-full w-full flex-col overflow-hidden bg-background">
      <div ref={titleBarRef}>
        <TitleBar activeView={activeView} onViewChange={setActiveView} />
      </div>

      <main className="min-h-0 flex-1 overflow-y-auto p-3">
        <div
          ref={mainRef}
          key={activeView}
          className={cn("animate-view-fade", activeView === "settings" && "h-full")}
        >
          {activeView === "main" ? (
            <div className="flex flex-col gap-3">
              {pendingUpdate && (
                <UpdateBanner
                  currentVersion={currentVersion}
                  version={pendingUpdate.version}
                  notes={pendingUpdate.notes}
                  onDismiss={() => setPendingUpdate(null)}
                />
              )}
              {showBanner && (
                <StatusBanner status={status} busy={busy} onAction={handleAction} />
              )}
              {muteError && (
                <div className="animate-view-in rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs font-medium text-destructive">
                  {muteError}
                </div>
              )}
              <MicController
                status={status}
                devices={displayDevices}
                deviceId={deviceId}
                onDeviceChange={handleDeviceChange}
                muted={muted}
                onMutedChange={handleMutedChange}
                volume={volume}
                onVolumeChange={handleVolumeChange}
              />
              {settingsReady && (
                <div className="flex gap-3">
                  <div className="flex-1">
                  <HotkeyCard
                      disabled={!hotkeysEnabled || descriptor.controlsDisabled}
                      chord={muteChord}
                      onChordChange={setMuteChord}
                      registerError={muteRegisterError}
                      mode={hotkeyMode}
                      onModeChange={(mode) => updateKeybindsSettings({ hotkeyMode: mode })}
                    />
                  </div>
                  <div className="flex-1">
                    <VuMeterCard
                      muted={muted}
                      disabled={descriptor.controlsDisabled}
                      level={inputLevel}
                      reduceMotion={reduceMotion}
                    />
                  </div>
                </div>
              )}
              <AppTrackerCard apps={trackedApps} />
            </div>
          ) : (
            <SettingsPanel
              systemSettings={{
                popupEnabled,
                popupLocation,
                soundEnabled,
                soundVolume,
                autoMuteEnabled,
                autoMuteMinutes,
              }}
              onSystemSettingsChange={updateSystemSettings}
              appearanceSettings={{ theme, accentColor, reduceMotion }}
              onAppearanceSettingsChange={updateAppearanceSettings}
              keybindsSettings={{ hotkeysEnabled, hotkeyMode, muteChord, openAppChord }}
              onKeybindsSettingsChange={updateKeybindsSettings}
              windowsSettings={{
                launchAtStartup,
                startupState,
                startupDelay,
                minimizeToTrayOnClose,
                rememberWindowPosition,
                alwaysOnTop,
              }}
              onWindowsSettingsChange={updateWindowsSettings}
              funModeUnlocked={funModeUnlocked}
              onFunModeUnlockedChange={setFunModeUnlocked}
              funModeEnabled={funModeEnabled}
              onFunModeEnabledChange={setFunModeEnabled}
              crazyColors={crazyColors}
              onCrazyColorsChange={setCrazyColors}
              randomMute={randomMute}
              onRandomMuteChange={setRandomMute}
              normalVolume={normalVolume}
              onNormalVolumeChange={setNormalVolume}
            />
          )}
        </div>
      </main>

      <div ref={statusBarRef}>
        <StatusBar status={status} muted={muted} />
      </div>
    </div>
  )
}