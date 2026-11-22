"use client"

import * as React from "react"
import { invoke } from "@tauri-apps/api/core"
import { Loader2, X } from "lucide-react"
import { cn } from "@/lib/utils"

interface UpdateBannerProps {
  currentVersion: string
  version: string
  notes: string
  onScheduleLater: () => void
  onDismiss: () => void
}

export function UpdateBanner({
  currentVersion,
  version,
  notes,
  onScheduleLater,
  onDismiss,
}: UpdateBannerProps) {
  const [installing, setInstalling] = React.useState(false)
  const [error, setError] = React.useState<string | null>(null)

  async function handleInstall() {
    setInstalling(true)
    setError(null)
    try {
      await invoke("install_update")
      // app restarts automatically on success so no cleanup needed
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
        <div className="flex shrink-0 items-center gap-1.5">
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
            onClick={onScheduleLater}
            disabled={installing}
            className="rounded-md border border-border/70 bg-background/60 px-2.5 py-1 text-xs font-medium text-foreground transition-colors hover:bg-background/90 disabled:opacity-50"
          >
            Install later
          </button>
          <button
            type="button"
            onClick={onDismiss}
            disabled={installing}
            aria-label="Dismiss update notice"
            title="Dismiss"
            className="flex size-6 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-background/60 hover:text-foreground disabled:opacity-50"
          >
            <X className="size-3.5" />
          </button>
        </div>
      </div>
    </div>
  )
}

export function ScheduledUpdateBanner({
  currentVersion,
  version,
  phase,
  onCancel,
}: {
  currentVersion: string
  version: string
  phase: string
  onCancel: () => void
}) {
  const statusText =
    phase === "waiting-network"
      ? "Waiting for network connection…"
      : phase === "installing"
        ? "Installing update & restarting…"
        : "Downloading scheduled update…"
  const canCancel = phase !== "installing"

  return (
    <div className="animate-view-in rounded-lg border border-primary/40 bg-primary/10 px-3 py-2.5">
      <div className="flex items-center justify-between gap-3">
        <div className="flex min-w-0 flex-1 items-center gap-2.5">
          <Loader2 className="size-3.5 shrink-0 animate-spin text-primary" />
          <div className="min-w-0 flex-1">
            <p className="text-xs font-semibold text-primary">
              {currentVersion ? `Updating v${currentVersion} → v${version}` : `Updating to v${version}`}
            </p>
            <p className="mt-0.5 truncate text-xs text-muted-foreground">{statusText}</p>
          </div>
        </div>
        {canCancel && (
          <button
            type="button"
            onClick={onCancel}
            className="shrink-0 rounded-md border border-border/70 bg-background/60 px-2.5 py-1 text-xs font-medium text-foreground transition-colors hover:bg-background/90"
          >
            Cancel
          </button>
        )}
      </div>
    </div>
  )
}