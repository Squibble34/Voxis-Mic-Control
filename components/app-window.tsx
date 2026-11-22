"use client"

import * as React from "react"
import { AudioWaveform } from "lucide-react"
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
import { ScheduledUpdateBanner, UpdateBanner } from "@/components/update-banner"
import { VirtualMicBanner, type VirtualMicStatus } from "@/components/virtual-mic-banner"
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
  DEFAULT_NOISE_SETTINGS,
  DEFAULT_PROFILES_CONFIG,
  DEFAULT_SYSTEM_SETTINGS,
  DEFAULT_WINDOWS_SETTINGS,
  getContrastForeground,
  THEME_PRIMARY_HEX,
  type AppearanceSettings,
  type KeybindsSettings,
  type NoiseEngineId,
  type NoiseSettings,
  type ProfileSnapshot,
  type ProfilesConfig,
  type SystemSettings,
  type VoiceToneId,
  type WindowsSettings,
} from "@/lib/settings-types"
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

  const [popupEnabled, setPopupEnabled] = React.useState(DEFAULT_SYSTEM_SETTINGS.popupEnabled)
  const [popupLocation, setPopupLocation] = React.useState(DEFAULT_SYSTEM_SETTINGS.popupLocation)
  const [talkingWhileMutedPopup, setTalkingWhileMutedPopup] = React.useState(
    DEFAULT_SYSTEM_SETTINGS.talkingWhileMutedPopup,
  )
  const [persistentPillEnabled, setPersistentPillEnabled] = React.useState(
    DEFAULT_SYSTEM_SETTINGS.persistentPillEnabled,
  )
  const [soundEnabled, setSoundEnabled] = React.useState(DEFAULT_SYSTEM_SETTINGS.soundEnabled)
  const [soundVolume, setSoundVolume] = React.useState(DEFAULT_SYSTEM_SETTINGS.soundVolume)
  const [autoMuteEnabled, setAutoMuteEnabled] = React.useState(DEFAULT_SYSTEM_SETTINGS.autoMuteEnabled)
  const [autoMuteMinutes, setAutoMuteMinutes] = React.useState(DEFAULT_SYSTEM_SETTINGS.autoMuteMinutes)

  function updateSystemSettings(patch: Partial<SystemSettings>) {
    if (patch.popupEnabled !== undefined) setPopupEnabled(patch.popupEnabled)
    if (patch.popupLocation !== undefined) setPopupLocation(patch.popupLocation)
    if (patch.talkingWhileMutedPopup !== undefined) setTalkingWhileMutedPopup(patch.talkingWhileMutedPopup)
    if (patch.persistentPillEnabled !== undefined) setPersistentPillEnabled(patch.persistentPillEnabled)
    if (patch.soundEnabled !== undefined) setSoundEnabled(patch.soundEnabled)
    if (patch.soundVolume !== undefined) setSoundVolume(patch.soundVolume)
    if (patch.autoMuteEnabled !== undefined) setAutoMuteEnabled(patch.autoMuteEnabled)
    if (patch.autoMuteMinutes !== undefined) setAutoMuteMinutes(patch.autoMuteMinutes)
  }

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

  const [noiseEngine, setNoiseEngine] = React.useState<NoiseEngineId>(DEFAULT_NOISE_SETTINGS.engine)
  const [attenLimDb, setAttenLimDb] = React.useState(DEFAULT_NOISE_SETTINGS.attenLimDb)
  const [postFilterBeta, setPostFilterBeta] = React.useState(DEFAULT_NOISE_SETTINGS.postFilterBeta)
  const [autoSetDefaultMic, setAutoSetDefaultMic] = React.useState(DEFAULT_NOISE_SETTINGS.autoSetDefaultMic)
  const [volumeBoostEnabled, setVolumeBoostEnabled] = React.useState(
    DEFAULT_NOISE_SETTINGS.volumeBoostEnabled,
  )
  const [highPassEnabled, setHighPassEnabled] = React.useState(
    DEFAULT_NOISE_SETTINGS.highPassEnabled,
  )
  const [voiceTone, setVoiceTone] = React.useState<VoiceToneId>(DEFAULT_NOISE_SETTINGS.voiceTone)
  const [noiseGateEnabled, setNoiseGateEnabled] = React.useState(
    DEFAULT_NOISE_SETTINGS.noiseGateEnabled,
  )
  const [noiseGateThreshold, setNoiseGateThreshold] = React.useState(
    DEFAULT_NOISE_SETTINGS.noiseGateThreshold,
  )
  const [compressorEnabled, setCompressorEnabled] = React.useState(
    DEFAULT_NOISE_SETTINGS.compressorEnabled,
  )
  const [profilesConfig, setProfilesConfig] = React.useState<ProfilesConfig>(DEFAULT_PROFILES_CONFIG)
  const [sidetoneEnabled, setSidetoneEnabled] = React.useState(false)

  function updateNoiseSettings(patch: Partial<NoiseSettings>) {
    if (patch.engine !== undefined) setNoiseEngine(patch.engine)
    if (patch.attenLimDb !== undefined) setAttenLimDb(patch.attenLimDb)
    if (patch.postFilterBeta !== undefined) setPostFilterBeta(patch.postFilterBeta)
    if (patch.autoSetDefaultMic !== undefined) setAutoSetDefaultMic(patch.autoSetDefaultMic)
    if (patch.volumeBoostEnabled !== undefined) setVolumeBoostEnabled(patch.volumeBoostEnabled)
    if (patch.highPassEnabled !== undefined) setHighPassEnabled(patch.highPassEnabled)
    if (patch.voiceTone !== undefined) setVoiceTone(patch.voiceTone)
    if (patch.noiseGateEnabled !== undefined) setNoiseGateEnabled(patch.noiseGateEnabled)
    if (patch.noiseGateThreshold !== undefined) setNoiseGateThreshold(patch.noiseGateThreshold)
    if (patch.compressorEnabled !== undefined) setCompressorEnabled(patch.compressorEnabled)
  }

  function handleSidetoneChange(enabled: boolean) {
    setSidetoneEnabled(enabled)
    invoke("set_sidetone_enabled", { enabled }).catch(console.error)
  }

  React.useEffect(() => {
    const unlistenMute = listen<{ muted: boolean }>("mute-changed", (event) => {
      lastLocalChange.current = Date.now()
      setMuted(event.payload.muted)
    })
    const unlistenPill = listen<{ enabled: boolean }>("persistent-pill-changed", (event) => {
      setPersistentPillEnabled(event.payload.enabled)
    })
    return () => {
      unlistenMute.then((unlisten) => unlisten())
      unlistenPill.then((unlisten) => unlisten())
    }
  }, [])

  const [pendingUpdate, setPendingUpdate] = React.useState<{ version: string; notes: string } | null>(null)
  const [scheduledUpdate, setScheduledUpdate] = React.useState<{ version: string; notes: string } | null>(null)
  const [scheduledProgress, setScheduledProgress] = React.useState<{ version: string; phase: string } | null>(null)
  const [virtualMicStatus, setVirtualMicStatus] = React.useState<VirtualMicStatus | null>(null)
  const [currentVersion, setCurrentVersion] = React.useState<string>("")

  React.useEffect(() => {
    getVersion().then(setCurrentVersion).catch(() => {})
    invoke<VirtualMicStatus>("get_virtual_mic_status")
      .then(setVirtualMicStatus)
      .catch(() => {})
    invoke<{ version: string; notes: string } | null>("get_pending_update")
      .then((info) => {
        if (info) setPendingUpdate(info)
      })
      .catch(() => {})
    invoke<{ version: string; notes: string } | null>("get_scheduled_update")
      .then((info) => {
        setScheduledUpdate(info)
      })
      .catch(() => {})
    invoke<{ version: string; phase: string } | null>("get_scheduled_update_progress")
      .then((progress) => {
        setScheduledProgress(progress)
      })
      .catch(() => {})
  }, [])

  React.useEffect(() => {
    const unlistenUpdate = listen<{ version: string; notes: string }>("update-available", (event) => {
      setPendingUpdate(event.payload)
    })
    const unlistenScheduled = listen<{ version: string; notes: string } | null>(
      "scheduled-update-changed",
      (event) => {
        setScheduledUpdate(event.payload)
      },
    )
    const unlistenProgress = listen<{ version: string; phase: string } | null>(
      "scheduled-update-progress",
      (event) => {
        setScheduledProgress(event.payload)
      },
    )
    const unlistenVirtualMic = listen<VirtualMicStatus>(
      "virtual-mic-status-changed",
      (event) => {
        setVirtualMicStatus(event.payload)
      },
    )
    if (process.env.NODE_ENV === "production") {
      return () => {
        unlistenUpdate.then((unlisten) => unlisten())
        unlistenScheduled.then((unlisten) => unlisten())
        unlistenProgress.then((unlisten) => unlisten())
        unlistenVirtualMic.then((unlisten) => unlisten())
      }
    }

    const unlistenClear = listen("dev-update-cleared", () => {
      setPendingUpdate(null)
    })
    const unlistenStatus = listen<MicStatus>("dev-status", (event) => {
      setDismissed(false)
      setStatus(event.payload)
    })
    const unlistenMuteErr = listen<string>("dev-mute-error", (event) => {
      setMuteError(event.payload)
      window.setTimeout(() => setMuteError(null), 3000)
    })
    const unlistenHotkeyErr = listen<boolean>("dev-hotkey-error", (event) => {
      setMuteRegisterError(event.payload)
    })
    return () => {
      unlistenUpdate.then((unlisten) => unlisten())
      unlistenScheduled.then((unlisten) => unlisten())
      unlistenProgress.then((unlisten) => unlisten())
      unlistenVirtualMic.then((unlisten) => unlisten())
      unlistenClear.then((unlisten) => unlisten())
      unlistenStatus.then((unlisten) => unlisten())
      unlistenMuteErr.then((unlisten) => unlisten())
      unlistenHotkeyErr.then((unlisten) => unlisten())
    }
  }, [])

  function handleScheduleUpdateLater() {
    if (!pendingUpdate) return
    invoke("schedule_update_for_reopen", {
      version: pendingUpdate.version,
      notes: pendingUpdate.notes,
    }).catch(console.error)
  }

  function handleCancelScheduledUpdate() {
    invoke("cancel_scheduled_update").catch(console.error)
  }

  function handleRetryUsbipSetup(forceReinstall: boolean) {
    invoke("retry_usbip_setup", { forceReinstall }).catch(console.error)
  }

  function handleOpenUsbipReleasePage() {
    invoke("open_usbip_release_page").catch(console.error)
  }

  const [inputLevel, setInputLevel] = React.useState<number | undefined>(undefined)
  const [talkingWhileMuted, setTalkingWhileMuted] = React.useState(false)
  React.useEffect(() => {
    if (!muted) setTalkingWhileMuted(false)
  }, [muted])

  React.useEffect(() => {
    const unlistenLevelPromise = listen<number>("input-level-changed", (event) => {
      setInputLevel(event.payload)
    })
    const unlistenTalkingPromise = listen<boolean>("talking-while-muted-state", (event) => {
      setTalkingWhileMuted(event.payload)
    })
    return () => {
      unlistenLevelPromise.then((unlisten) => unlisten())
      unlistenTalkingPromise.then((unlisten) => unlisten())
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
  const [devicesLoaded, setDevicesLoaded] = React.useState(false)
  const [bootSplashVisible, setBootSplashVisible] = React.useState(true)
  const [bootSplashMounted, setBootSplashMounted] = React.useState(true)
  // tracks the last time the user changed mute or volume so the background sync below doesnt fight a local change
  const lastLocalChange = React.useRef(0)

  React.useEffect(() => {
    if (!settingsReady || !devicesLoaded) return
    const fadeTimer = window.setTimeout(() => {
      setBootSplashVisible(false)
    }, 180)
    const unmountTimer = window.setTimeout(() => {
      setBootSplashMounted(false)
    }, 480)
    return () => {
      window.clearTimeout(fadeTimer)
      window.clearTimeout(unmountTimer)
    }
  }, [settingsReady, devicesLoaded])

  const syncDeviceState = React.useCallback((id: string) => {
    if (!id) return
    invoke<boolean>("get_mute_state", { deviceId: id })
      .then(setMuted)
      .catch(console.error)
    invoke<number>("get_current_volume", { deviceId: id })
      .then((level) => setVolume(Math.round(level * 100)))
      .catch(console.error)
  }, [])

  React.useEffect(() => {
    const unlistenDevice = listen<{ deviceId: string }>("device-changed", (event) => {
      const nextId = event.payload.deviceId
      setDeviceId(nextId)
      syncDeviceState(nextId)
      storeRef.current?.set("lastDeviceId", nextId)
      storeRef.current?.save().catch(console.error)
    })
    const unlistenProfile = listen<{ profileId?: string | null; snapshot: ProfileSnapshot }>(
      "profile-applied",
      (event) => {
        const snap = event.payload.snapshot
        if (snap.systemSettings) updateSystemSettings(snap.systemSettings)
        if (snap.appearanceSettings) updateAppearanceSettings(snap.appearanceSettings)
        if (snap.keybindsSettings) updateKeybindsSettings(snap.keybindsSettings)
        if (snap.windowsSettings) updateWindowsSettings(snap.windowsSettings)
        if (snap.noiseSettings) updateNoiseSettings(snap.noiseSettings)
        if (typeof snap.softwareGain === "number") {
          setVolume(Math.round(snap.softwareGain * 100))
          storeRef.current?.set("softwareGain", snap.softwareGain)
        }
        if (event.payload.profileId !== undefined) {
          setProfilesConfig((prev) => ({
            ...prev,
            activeProfileId: event.payload.profileId ?? null,
          }))
        }
      },
    )
    return () => {
      unlistenDevice.then((unlisten) => unlisten())
      unlistenProfile.then((unlisten) => unlisten())
    }
  }, [syncDeviceState])

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
          store.get<NoiseSettings>("noiseSettings"),
          store.get<ProfilesConfig>("profilesConfig"),
        ])
      })
      .then(([savedDeviceId, savedSystemSettings, savedAppearanceSettings, savedKeybindsSettings, savedWindowsSettings, savedNoiseSettings, savedProfilesConfig]) => {
        savedDeviceIdRef.current = savedDeviceId ?? null

        const system = { ...DEFAULT_SYSTEM_SETTINGS, ...savedSystemSettings }
        setPopupEnabled(system.popupEnabled)
        setPopupLocation(system.popupLocation)
        setTalkingWhileMutedPopup(system.talkingWhileMutedPopup)
        setPersistentPillEnabled(system.persistentPillEnabled)
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

        const noise = { ...DEFAULT_NOISE_SETTINGS, ...savedNoiseSettings }
        setNoiseEngine(noise.engine)
        setAttenLimDb(noise.attenLimDb)
        setPostFilterBeta(noise.postFilterBeta)
        setAutoSetDefaultMic(noise.autoSetDefaultMic)
        setVolumeBoostEnabled(noise.volumeBoostEnabled)
        setHighPassEnabled(noise.highPassEnabled)
        setVoiceTone(noise.voiceTone)
        setNoiseGateEnabled(noise.noiseGateEnabled)
        setNoiseGateThreshold(noise.noiseGateThreshold)
        setCompressorEnabled(noise.compressorEnabled)

        if (savedProfilesConfig) {
          setProfilesConfig({ ...DEFAULT_PROFILES_CONFIG, ...savedProfilesConfig })
        }
      })
      .catch((err) => console.error("Failed to load settings:", err))
      .finally(() => setSettingsReady(true))
  }, [])

  React.useEffect(() => {
    if (!settingsReady) return
    const system: SystemSettings = {
      popupEnabled,
      popupLocation,
      talkingWhileMutedPopup,
      persistentPillEnabled,
      soundEnabled,
      soundVolume,
      autoMuteEnabled,
      autoMuteMinutes,
    }
    storeRef.current?.set("systemSettings", system)
    storeRef.current?.save().catch(console.error)
    invoke("set_persistent_pill_enabled", { enabled: persistentPillEnabled }).catch(console.error)
    invoke("set_sound_settings", { enabled: soundEnabled, volume: soundVolume }).catch(console.error)
    invoke("set_auto_mute_enabled", { enabled: autoMuteEnabled }).catch(console.error)
    invoke("set_auto_mute_idle_minutes", { minutes: autoMuteMinutes }).catch(console.error)
    invoke("set_talking_popup_enabled", { enabled: talkingWhileMutedPopup }).catch(console.error)
  }, [
    settingsReady,
    popupEnabled,
    popupLocation,
    talkingWhileMutedPopup,
    persistentPillEnabled,
    soundEnabled,
    soundVolume,
    autoMuteEnabled,
    autoMuteMinutes,
  ])

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

  React.useEffect(() => {
    if (!settingsReady) return
    const noise: NoiseSettings = {
      engine: noiseEngine,
      attenLimDb,
      postFilterBeta,
      autoSetDefaultMic,
      volumeBoostEnabled,
      highPassEnabled,
      voiceTone,
      noiseGateEnabled,
      noiseGateThreshold,
      compressorEnabled,
    }
    storeRef.current?.set("noiseSettings", noise)
    storeRef.current?.save().catch(console.error)
    invoke("set_noise_engine", { engine: noiseEngine }).catch(console.error)
    invoke("set_noise_atten_lim", { limDb: attenLimDb }).catch(console.error)
    invoke("set_noise_post_filter", { beta: postFilterBeta }).catch(console.error)
    invoke("set_auto_default_mic", { enabled: autoSetDefaultMic }).catch(console.error)
    invoke("set_high_pass_enabled", { enabled: highPassEnabled }).catch(console.error)
    invoke("set_voice_tone", { tone: voiceTone }).catch(console.error)
    invoke("set_noise_gate_enabled", { enabled: noiseGateEnabled }).catch(console.error)
    invoke("set_noise_gate_threshold", { threshold: noiseGateThreshold / 100.0 }).catch(console.error)
    invoke("set_compressor_enabled", { enabled: compressorEnabled }).catch(console.error)
  }, [
    settingsReady,
    noiseEngine,
    attenLimDb,
    postFilterBeta,
    autoSetDefaultMic,
    volumeBoostEnabled,
    highPassEnabled,
    voiceTone,
    noiseGateEnabled,
    noiseGateThreshold,
    compressorEnabled,
  ])

  React.useEffect(() => {
    if (!settingsReady) return
    storeRef.current?.set("profilesConfig", profilesConfig)
    storeRef.current?.save().catch(console.error)
    invoke("sync_profiles_config", { config: profilesConfig }).catch(console.error)
  }, [settingsReady, profilesConfig])

  const handleApplyProfile = React.useCallback((profileId: string) => {
    invoke("apply_profile", { profileId }).catch(console.error)
  }, [])

  React.useEffect(() => {
    if (!settingsReady || !rememberWindowPosition) return

    let debounceId: ReturnType<typeof setTimeout> | null = null

    const unlistenPromise = getCurrentWindow().onMoved((event) => {
      const { x, y } = event.payload
      // windows reports -32000 when a window is minimized to the taskbar
      if (x <= -10000 || y <= -10000) return
      if (debounceId) clearTimeout(debounceId)
      debounceId = setTimeout(() => {
        storeRef.current?.set("windowPosition", { x, y })
        storeRef.current?.save().catch(console.error)
      }, 500)
    })

    return () => {
      if (debounceId) clearTimeout(debounceId)
      unlistenPromise.then((unlisten) => unlisten())
    }
  }, [settingsReady, rememberWindowPosition])

  // kept out of the persist effect so unrelated windows settings never touch os autostart
  // checks real os state before disabling since calling disable() unconditionally threw in dev mode
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
    invoke("set_always_on_top", { enabled: alwaysOnTop }).catch(console.error)
  }, [settingsReady, alwaysOnTop])

  React.useEffect(() => {
    if (!settingsReady) return
    invoke("set_tray_icon", { accentColor: accentColor ?? THEME_PRIMARY_HEX[theme] }).catch(console.error)
  }, [settingsReady, theme, accentColor])

  // the popup is a separate window and cant see this components state, so tell it about popup relevant changes over an event instead
  React.useEffect(() => {
    if (!settingsReady) return
    emit("system-settings-changed", {
      popupEnabled,
      popupLocation,
      talkingWhileMutedPopup,
    }).catch(console.error)
  }, [settingsReady, popupEnabled, popupLocation, talkingWhileMutedPopup])

  React.useEffect(() => {
    if (!settingsReady) return
    emit("appearance-settings-changed", { theme, accentColor, reduceMotion }).catch(console.error)
  }, [settingsReady, theme, accentColor, reduceMotion])

  const applyDeviceList = React.useCallback(
    (devs: Device[]) => {
      setDevices(devs)
      setDevicesLoaded(true)

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

        if (Date.now() - lastLocalChange.current > 2500) {
          syncDeviceState(currentId)
        }

        return currentId
      })
    },
    [syncDeviceState],
  )

  const refreshDevices = React.useCallback(() => {
    invoke<Device[]>("get_audio_devices")
      .then(applyDeviceList)
      .catch((err) => {
        console.error(err)
        setDevicesLoaded(true)
      })
  }, [applyDeviceList])

  React.useEffect(() => {
    if (!settingsReady) return
    refreshDevices()
    const unlistenPromise = listen<Device[]>("audio-devices-changed", (event) => {
      applyDeviceList(event.payload)
    })
    return () => {
      unlistenPromise.then((unlisten) => unlisten())
    }
  }, [refreshDevices, applyDeviceList, settingsReady])

  const handleMutedChange = React.useCallback(
    (newMuted: boolean) => {
      lastLocalChange.current = Date.now()
      const previous = muted
      setMuted(newMuted)
      setMuteError(null)

      invoke("set_mute_state", { deviceId, muted: newMuted }).catch((err) => {
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
        const gain = newVolume / 100.0
        invoke("set_volume", { deviceId, level: gain }).catch(console.error)
        storeRef.current?.set("softwareGain", gain)
        storeRef.current?.save().catch(console.error)
      }, 80)
    },
    [deviceId],
  )

  React.useEffect(() => {
    if (!settingsReady) return
    if (!volumeBoostEnabled && volume > 100) {
      handleVolumeChange(100)
    }
  }, [settingsReady, volumeBoostEnabled, volume, handleVolumeChange])

  const descriptor = MIC_STATUS[status]
  const [muteRegisterError, setMuteRegisterError] = React.useState(false)

  React.useEffect(() => {
    if (!settingsReady) return
    let cancelled = false
    invoke<boolean>("sync_hotkey_settings", {
      config: {
        hotkeysEnabled,
        hotkeyMode,
        muteChord,
        openAppChord,
      },
    })
      .then((hasError) => {
        if (!cancelled) setMuteRegisterError(hasError)
      })
      .catch(console.error)
    return () => {
      cancelled = true
    }
  }, [settingsReady, hotkeysEnabled, hotkeyMode, muteChord, openAppChord])

  // switching hotkey mode mutes so push-to-talk starts from its resting state
  // skip the initial load run so restoring a saved push-to-talk mode on startup doesnt force a mute
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

  // kept here instead of settingspanel (and unpersisted) so toggling fun mode doesnt reset when you leave settings
  const [funModeUnlocked, setFunModeUnlocked] = React.useState(false)
  const [funModeEnabled, setFunModeEnabled] = React.useState(false)
  const [crazyColors, setCrazyColors] = React.useState(false)
  const [randomMute, setRandomMute] = React.useState(false)
  const [normalVolume, setNormalVolume] = React.useState(false)
  const [funnyMic, setFunnyMic] = React.useState(false)

  React.useEffect(() => {
    invoke("set_funny_mic_enabled", { enabled: funModeEnabled && funnyMic }).catch(console.error)
  }, [funModeEnabled, funnyMic])

  const accentColorRef = React.useRef(accentColor)
  React.useEffect(() => {
    accentColorRef.current = accentColor
  }, [accentColor])

  // empty deps keep a stable identity so unrelated renders dont tear down the effect below and cause flashing
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

  // refs stop handlemutedchange identity changes on each mute toggle from resetting the random mute timer
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
    contentPaddingY: 24, // main has p-3 padding (24px total) that scrollheight on the inner ref wont include
    reduceMotion,
    viewKey: activeView,
  })

  return (
    <div className="relative flex h-full w-full flex-col overflow-hidden bg-background">
      <div ref={titleBarRef}>
        <TitleBar activeView={activeView} onViewChange={setActiveView} />
      </div>

      {bootSplashMounted && (
        <div
          aria-hidden={!bootSplashVisible}
          className={cn(
            "absolute inset-x-0 top-[38px] bottom-0 z-50 flex flex-col items-center justify-center gap-3 bg-background transition-opacity duration-300 ease-out select-none",
            bootSplashVisible ? "opacity-100" : "pointer-events-none opacity-0",
          )}
        >
          <div className="relative flex size-14 items-center justify-center rounded-2xl border border-primary/35 bg-primary/10 text-primary shadow-[0_0_28px_-8px_var(--primary)]">
            <AudioWaveform className="size-6 animate-pulse" />
          </div>
          <div className="flex flex-col items-center gap-1">
            <span className="text-xs font-semibold tracking-[0.16em] text-foreground/90 uppercase">
              Voxis
            </span>
            <span className="text-[11px] text-muted-foreground">
              Initializing audio pipeline…
            </span>
          </div>
        </div>
      )}

      <main className="min-h-0 flex-1 overflow-y-auto p-3">
        <div
          ref={mainRef}
          key={activeView}
          className={cn("animate-view-fade", activeView === "settings" && "h-full")}
        >
          {activeView === "main" ? (
            <div className="flex flex-col gap-3">
              {scheduledProgress ? (
                <ScheduledUpdateBanner
                  currentVersion={currentVersion}
                  version={scheduledProgress.version}
                  phase={scheduledProgress.phase}
                  onCancel={handleCancelScheduledUpdate}
                />
              ) : (
                pendingUpdate &&
                !scheduledUpdate && (
                  <UpdateBanner
                    currentVersion={currentVersion}
                    version={pendingUpdate.version}
                    notes={pendingUpdate.notes}
                    onScheduleLater={handleScheduleUpdateLater}
                    onDismiss={() => setPendingUpdate(null)}
                  />
                )
              )}
              {showBanner && (
                <StatusBanner status={status} busy={busy} onAction={handleAction} />
              )}
              <VirtualMicBanner
                virtualMicStatus={virtualMicStatus}
                onRetry={handleRetryUsbipSetup}
                onOpenManualDownload={handleOpenUsbipReleasePage}
              />
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
                maxVolume={volumeBoostEnabled ? 200 : 100}
                noiseEngine={noiseEngine}
                onNoiseEngineChange={(engine) => updateNoiseSettings({ engine })}
              />
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
                    listening={sidetoneEnabled}
                    onListeningChange={handleSidetoneChange}
                    talkingWhileMuted={talkingWhileMuted}
                  />
                </div>
              </div>
              <AppTrackerCard apps={trackedApps} />
            </div>
          ) : (
            <SettingsPanel
              systemSettings={{
                popupEnabled,
                popupLocation,
                talkingWhileMutedPopup,
                persistentPillEnabled,
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
              noiseSettings={{
                engine: noiseEngine,
                attenLimDb,
                postFilterBeta,
                autoSetDefaultMic,
                volumeBoostEnabled,
                highPassEnabled,
                voiceTone,
                noiseGateEnabled,
                noiseGateThreshold,
                compressorEnabled,
              }}
              onNoiseSettingsChange={updateNoiseSettings}
              softwareGain={volume / 100}
              profilesConfig={profilesConfig}
              onProfilesConfigChange={setProfilesConfig}
              onApplyProfile={handleApplyProfile}
              trackedApps={trackedApps}
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
              funnyMic={funnyMic}
              onFunnyMicChange={setFunnyMic}
              pendingUpdate={pendingUpdate}
              onPendingUpdateChange={setPendingUpdate}
              scheduledUpdate={scheduledUpdate}
              scheduledProgress={scheduledProgress}
              onCancelScheduledUpdate={handleCancelScheduledUpdate}
              virtualMicStatus={virtualMicStatus}
              onRetryUsbipSetup={handleRetryUsbipSetup}
            />
          )}
        </div>
      </main>

      <div ref={statusBarRef}>
        <StatusBar
          status={status}
          muted={muted}
          version={currentVersion}
          virtualMicState={virtualMicStatus?.state}
        />
      </div>
    </div>
  )
}