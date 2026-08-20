"use client"

import * as React from "react"
import { MicIcon, type MicIconHandle } from "@/components/icons/mic-icon"
import { MicOffIcon, type MicOffIconHandle } from "@/components/icons/mic-off-icon"
import { getCurrentWindow, primaryMonitor } from "@tauri-apps/api/window"
import { LogicalPosition } from "@tauri-apps/api/dpi"
import { listen } from "@tauri-apps/api/event"
import { load } from "@tauri-apps/plugin-store"
import { Card } from "@/components/ui/card"
import { cn } from "@/lib/utils"
import {
  DEFAULT_APPEARANCE_SETTINGS,
  DEFAULT_SYSTEM_SETTINGS,
  getContrastForeground,
  type AppearanceSettings,
  type SystemSettings,
} from "@/lib/settings-types"

const MARGIN = 16
const POPUP_SIZE = 96
const VISIBLE_MS = 2000
const FADE_MS = 200

export default function PopupPage() {
  const [muted, setMuted] = React.useState(false)
  const [visible, setVisible] = React.useState(false)
  const [popKey, setPopKey] = React.useState(0)
  const hideTimeout = React.useRef<ReturnType<typeof setTimeout> | null>(null)
  const lastLocation = React.useRef<string | null>(null)
  const micRef = React.useRef<MicIconHandle>(null)
  const micOffRef = React.useRef<MicOffIconHandle>(null)
  const popupEnabledRef = React.useRef(DEFAULT_SYSTEM_SETTINGS.popupEnabled)
  const popupLocationRef = React.useRef(DEFAULT_SYSTEM_SETTINGS.popupLocation)
  const reduceMotionRef = React.useRef(DEFAULT_APPEARANCE_SETTINGS.reduceMotion)

  function applyAppearance(appearance: AppearanceSettings) {
    const root = document.documentElement
    root.classList.toggle("dark", appearance.theme === "dark")
    root.classList.toggle("light", appearance.theme === "light")
    if (appearance.accentColor) {
      root.style.setProperty("--primary", appearance.accentColor)
      root.style.setProperty("--primary-foreground", getContrastForeground(appearance.accentColor))
    } else {
      root.style.removeProperty("--primary")
      root.style.removeProperty("--primary-foreground")
    }
    root.classList.toggle("reduce-motion", appearance.reduceMotion)
    reduceMotionRef.current = appearance.reduceMotion
  }

  React.useEffect(() => {
    load("settings.json", { autoSave: false })
      .then((store) =>
        Promise.all([
          store.get<SystemSettings>("systemSettings"),
          store.get<AppearanceSettings>("appearanceSettings"),
        ]),
      )
      .then(([savedSystem, savedAppearance]) => {
        if (savedSystem) {
          popupEnabledRef.current = savedSystem.popupEnabled
          popupLocationRef.current = savedSystem.popupLocation
        }
        applyAppearance({ ...DEFAULT_APPEARANCE_SETTINGS, ...savedAppearance })
      })
      .catch((err) => console.error("Failed to load popup settings:", err))

    const unlistenSystemPromise = listen<{ popupEnabled: boolean; popupLocation: string }>(
      "system-settings-changed",
      (event) => {
        popupEnabledRef.current = event.payload.popupEnabled
        popupLocationRef.current = event.payload.popupLocation
      },
    )
    const unlistenAppearancePromise = listen<AppearanceSettings>(
      "appearance-settings-changed",
      (event) => applyAppearance(event.payload),
    )
    return () => {
      unlistenSystemPromise.then((unlisten) => unlisten())
      unlistenAppearancePromise.then((unlisten) => unlisten())
    }
  }, [])

  React.useEffect(() => {
    const win = getCurrentWindow()

    async function positionFor(location: string) {
      const monitor = await primaryMonitor()
      if (!monitor) return
      const scale = monitor.scaleFactor
      const screenX = monitor.position.x / scale
      const screenY = monitor.position.y / scale
      const screenWidth = monitor.size.width / scale
      const screenHeight = monitor.size.height / scale

      const x =
        location === "top-left" || location === "bottom-left"
          ? screenX + MARGIN
          : screenX + screenWidth - POPUP_SIZE - MARGIN
      const y =
        location === "top-left" || location === "top-right"
          ? screenY + MARGIN
          : screenY + screenHeight - POPUP_SIZE - MARGIN

      await win.setPosition(new LogicalPosition(x, y))
      lastLocation.current = location
    }

    const unlistenPromise = listen<{ muted: boolean }>("mute-changed", async (event) => {
      if (!popupEnabledRef.current) return
      if (hideTimeout.current) clearTimeout(hideTimeout.current)

      if (lastLocation.current !== popupLocationRef.current) {
        await positionFor(popupLocationRef.current)
      }
      setMuted(event.payload.muted)
      setPopKey((k) => k + 1)
      await win.show()
      requestAnimationFrame(() => {
        setVisible(true)
        if (reduceMotionRef.current) return
        if (event.payload.muted) {
          micOffRef.current?.startAnimation()
        } else {
          micRef.current?.startAnimation()
        }
      })

      hideTimeout.current = setTimeout(() => {
        setVisible(false)
        setTimeout(() => win.hide(), FADE_MS)
      }, VISIBLE_MS)
    })

    return () => {
      unlistenPromise.then((unlisten) => unlisten())
      if (hideTimeout.current) clearTimeout(hideTimeout.current)
    }
  }, [])

  return (
    <div className="flex h-screen w-screen items-center justify-center bg-transparent">
      <style>{`html, body { background: transparent !important; }`}</style>
      <Card
      key={popKey}
      className={cn(
        "flex size-16 items-center justify-center rounded-2xl border-2 bg-card py-0 transition-all duration-200 ease-out",
        visible ? "scale-100 opacity-100" : "scale-90 opacity-0",
        muted
        ? "border-destructive/50 shadow-[0_0_10px_1px_color-mix(in_oklch,var(--destructive)_45%,transparent)]"
          : cn("border-primary/50", !reduceMotionRef.current && "animate-pulse-once"),
      )}
    >
        {muted ? (
          <MicOffIcon ref={micOffRef} size={28} className="text-destructive" />
        ) : (
          <MicIcon ref={micRef} size={28} className="text-primary" />
        )}
      </Card>
    </div>
  )
}