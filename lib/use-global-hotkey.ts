"use client"

import * as React from "react"
import { register, unregister } from "@tauri-apps/plugin-global-shortcut"

type HotkeyMode = "toggle" | "push-to-talk"

// unmounting this hook unregisters the shortcut, so it needs to live in something that stays mounted for the app's whole lifetime
export function useGlobalHotkey(
  chord: string[],
  enabled: boolean,
  onTrigger: () => void,
  options?: { mode?: HotkeyMode; onRelease?: () => void },
) {
  const mode = options?.mode ?? "toggle"

  const onTriggerRef = React.useRef(onTrigger)
  React.useEffect(() => {
    onTriggerRef.current = onTrigger
  }, [onTrigger])

  const onReleaseRef = React.useRef(options?.onRelease)
  React.useEffect(() => {
    onReleaseRef.current = options?.onRelease
  }, [options?.onRelease])

  // needed so os auto-repeat during a push-to-talk hold only triggers once, until the release event resets it
  const heldRef = React.useRef(false)

  const [registerError, setRegisterError] = React.useState(false)

  React.useEffect(() => {
    if (!enabled || chord.length === 0) {
      setRegisterError(false)
      return
    }

    const shortcutStr = chord.join("+")
    let cancelled = false
    let lastFired = 0
    heldRef.current = false

    const setupShortcut = async () => {
      try {
        await unregister(shortcutStr).catch(() => {})
        if (cancelled) return
        await register(shortcutStr, (event) => {
          if (mode === "push-to-talk") {
            if (event.state === "Pressed") {
              if (heldRef.current) return
              heldRef.current = true
              onTriggerRef.current?.()
            } else if (event.state === "Released") {
              heldRef.current = false
              onReleaseRef.current?.()
            }
            return
          }

          if (event.state !== "Pressed") return
          const now = Date.now()
          if (now - lastFired < 50) return // debounce against os key-repeat
          lastFired = now
          onTriggerRef.current?.()
        })
        if (!cancelled) setRegisterError(false)
      } catch (err) {
        if (!cancelled) {
          console.error("Failed to register shortcut:", err)
          setRegisterError(true)
        }
      }
    }

    setupShortcut()

    return () => {
      cancelled = true
      heldRef.current = false
      unregister(shortcutStr).catch(() => {})
    }
  }, [chord, enabled, mode])

  return { registerError }
}