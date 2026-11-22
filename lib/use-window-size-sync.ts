"use client"

import * as React from "react"
import { getCurrentWindow } from "@tauri-apps/api/window"
import { LogicalSize } from "@tauri-apps/api/dpi"

const RESIZE_DURATION_MS = 280

function easeOutCubic(t: number) {
  return 1 - Math.pow(1 - t, 3)
}

// settings view sets one fixed height on entry with no observer so switching sidebar sections never re-triggers a resize
export function useWindowSizeSync({
  titleBarRef,
  contentRef,
  statusBarRef,
  width,
  settingsHeight,
  contentPaddingY,
  reduceMotion,
  viewKey,
}: {
  titleBarRef: React.RefObject<HTMLElement | null>
  contentRef: React.RefObject<HTMLElement | null>
  statusBarRef: React.RefObject<HTMLElement | null>
  width: number
  settingsHeight: number
  contentPaddingY: number
  reduceMotion: boolean
  viewKey: "main" | "settings"
}) {
  const lastHeight = React.useRef<number | null>(null)
  const animationFrame = React.useRef<number | null>(null)
  const isAnimating = React.useRef(false)

  const applySize = React.useCallback(
    (targetHeight: number) => {
      const win = getCurrentWindow()
      const currentViewportHeight = Math.round(window.innerHeight)
      const from = lastHeight.current ?? currentViewportHeight

      if (animationFrame.current) {
        window.cancelAnimationFrame(animationFrame.current)
        animationFrame.current = null
      }
      isAnimating.current = false

      win
        .isMinimized()
        .then((minimized) => {
          // windows ignores or corrupts setsize while minimized, defer until restored
          if (minimized) return

          lastHeight.current = targetHeight

          if (reduceMotion || from === targetHeight) {
            if (Math.round(window.innerHeight) !== targetHeight) {
              win.setSize(new LogicalSize(width, targetHeight)).catch(console.error)
            }
            return
          }

          isAnimating.current = true
          const start = performance.now()
          const step = async () => {
            const elapsed = performance.now() - start
            const t = Math.min(1, elapsed / RESIZE_DURATION_MS)
            const height = Math.round(from + (targetHeight - from) * easeOutCubic(t))
            try {
              await win.setSize(new LogicalSize(width, height))
            } catch (err) {
              console.error(err)
            }
            // waits for this resize to actually land before queuing the next one, otherwise calls stack up and the window tears
            if (t < 1) {
              animationFrame.current = window.requestAnimationFrame(step)
            } else {
              isAnimating.current = false
              animationFrame.current = null
            }
          }
          animationFrame.current = window.requestAnimationFrame(step)
        })
        .catch(console.error)
    },
    [width, reduceMotion],
  )

  React.useEffect(() => {
    if (viewKey !== "settings") return
    applySize(settingsHeight)
  }, [viewKey, settingsHeight, applySize])

  React.useEffect(() => {
    if (viewKey !== "main") return
    const contentEl = contentRef.current
    if (!contentEl) return

    const measureAndApply = () => {
      const titleH = titleBarRef.current?.getBoundingClientRect().height ?? 0
      const statusH = statusBarRef.current?.getBoundingClientRect().height ?? 0
      const contentH = Math.max(
        contentEl.scrollHeight,
        contentEl.getBoundingClientRect().height,
      )
      applySize(Math.ceil(titleH + statusH + contentPaddingY + contentH))
    }

    measureAndApply()
    const observer = new ResizeObserver(measureAndApply)
    observer.observe(contentEl)

    let cancelled = false
    document.fonts?.ready.then(() => {
      if (!cancelled) measureAndApply()
    })

    let settleTimer: number | null = null
    const scheduleSettleCheck = () => {
      if (isAnimating.current) return
      if (settleTimer) window.clearTimeout(settleTimer)
      settleTimer = window.setTimeout(measureAndApply, 80)
    }

    window.addEventListener("focus", scheduleSettleCheck)
    window.addEventListener("resize", scheduleSettleCheck)
    document.addEventListener("visibilitychange", scheduleSettleCheck)

    return () => {
      cancelled = true
      if (settleTimer) window.clearTimeout(settleTimer)
      observer.disconnect()
      window.removeEventListener("focus", scheduleSettleCheck)
      window.removeEventListener("resize", scheduleSettleCheck)
      document.removeEventListener("visibilitychange", scheduleSettleCheck)
    }
  }, [viewKey, contentRef, titleBarRef, statusBarRef, contentPaddingY, applySize])

  React.useEffect(() => {
    return () => {
      if (animationFrame.current) window.cancelAnimationFrame(animationFrame.current)
    }
  }, [])
}