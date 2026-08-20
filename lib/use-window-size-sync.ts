"use client"

import * as React from "react"
import { getCurrentWindow } from "@tauri-apps/api/window"
import { LogicalSize } from "@tauri-apps/api/dpi"

const RESIZE_DURATION_MS = 280

function easeOutCubic(t: number) {
  return 1 - Math.pow(1 - t, 3)
}

// settings view uses one fixed height set once on entry with no observer, so switching sidebar
// sections can never re-trigger a resize like the main view does
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

  const applySize = React.useCallback(
    (targetHeight: number) => {
      const win = getCurrentWindow()
      const from = lastHeight.current ?? targetHeight
      lastHeight.current = targetHeight

      if (animationFrame.current) window.cancelAnimationFrame(animationFrame.current)

      if (reduceMotion || from === targetHeight) {
        win.setSize(new LogicalSize(width, targetHeight)).catch(console.error)
        return
      }

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
        if (t < 1) animationFrame.current = window.requestAnimationFrame(step)
      }
      animationFrame.current = window.requestAnimationFrame(step)
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
        const titleH = titleBarRef.current?.offsetHeight ?? 0
        const statusH = statusBarRef.current?.offsetHeight ?? 0
        applySize(titleH + statusH + contentPaddingY + contentEl.scrollHeight)
      }

    measureAndApply()
    const observer = new ResizeObserver(measureAndApply)
    observer.observe(contentEl)
    return () => observer.disconnect()
  }, [viewKey, contentRef, titleBarRef, statusBarRef, applySize])

  React.useEffect(() => {
    return () => {
      if (animationFrame.current) window.cancelAnimationFrame(animationFrame.current)
    }
  }, [])
}