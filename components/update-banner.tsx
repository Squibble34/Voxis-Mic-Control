"use client"

import * as React from "react"
import { invoke } from "@tauri-apps/api/core"
import { cn } from "@/lib/utils"

interface UpdateBannerProps {
  currentVersion: string
  version: string
  notes: string
  onDismiss: () => void
}

export function UpdateBanner({ currentVersion, version, notes, onDismiss }: UpdateBannerProps) {
  const [installing, setInstalling] = React.useState(false)
  const [error, setError] = React.useState<string | null>(null)

  async function handleInstall() {
    setInstalling(true)
    setError(null)
    try {
      await invoke("install_update")
      // app restarts automatically on success — nothing to do here
    } catch (e) {
      setError(String(e))
      setInstalling(false)
    }
  }

  return (
    <div className="animate-view-in rounded-lg border border-primary/40 bg-primary/10 px-3 py-2.5">
      <div className="flex items-center justify-between gap-3">
        <div className="min-w-0 flex-1">
          <p className="text-xs font-semibold text-primary">
            {currentVersion ? `v${currentVersion} → v${version}` : `Update available: v${version}`}
          </p>
          {notes && (
            <p className="mt-0.5 truncate text-xs text-muted-foreground">{notes}</p>
          )}
          {error && (
            <p className="mt-0.5 text-xs text-destructive">{error}</p>
          )}
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <button
            onClick={handleInstall}
            disabled={installing}
            className={cn(
              "rounded-md bg-primary px-2.5 py-1 text-xs font-medium text-primary-foreground transition-opacity",
              installing ? "opacity-50 cursor-not-allowed" : "hover:opacity-90"
            )}
          >
            {installing ? "Downloading…" : "Download & Install"}
          </button>
          <button
            onClick={onDismiss}
            disabled={installing}
            className="text-xs text-muted-foreground hover:text-foreground disabled:opacity-50"
          >
            Dismiss
          </button>
        </div>
      </div>
    </div>
  )
}