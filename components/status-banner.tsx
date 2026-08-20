"use client"

import {
  AlertTriangle,
  ExternalLink,
  Info,
  Loader2,
  RotateCcw,
  ShieldAlert,
  Unplug,
  X,
} from "lucide-react"

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { MIC_STATUS, type MicStatus } from "@/lib/mic-status"
import { cn } from "@/lib/utils"

const ICONS = {
  reconnecting: Loader2,
  disconnected: Unplug,
  "permission-denied": ShieldAlert,
  "no-devices": AlertTriangle,
  "in-use": AlertTriangle,
  "volume-unsupported": Info,
} as const

const SEVERITY_STYLES = {
  ok: "",
  info: "border-border/70 bg-muted/40 text-foreground",
  warning: "border-warning/45 bg-warning/10 text-foreground",
  error: "border-destructive/45 bg-destructive/10 text-destructive",
} as const

export function StatusBanner({
  status,
  busy,
  onAction,
}: {
  status: MicStatus
  busy?: boolean
  onAction: () => void
}) {
  const descriptor = MIC_STATUS[status]
  if (!descriptor.title) return null

  const Icon = ICONS[status as keyof typeof ICONS] ?? Info

  return (
    <Alert
      aria-live="polite"
      className={cn(
        "animate-view-in items-start pr-2",
        SEVERITY_STYLES[descriptor.severity],
      )}
    >
      <Icon className={cn(status === "reconnecting" && "animate-spin")} />
      <AlertTitle className="text-[13px]">{descriptor.title}</AlertTitle>
      <AlertDescription
        className={cn(
          "text-xs leading-relaxed",
          descriptor.severity === "error" && "text-destructive/85",
        )}
      >
        {descriptor.description}
      </AlertDescription>
      {descriptor.actionLabel && (
        <div className="col-start-2 mt-1.5 flex">
          <Button
            variant="outline"
            size="sm"
            onClick={onAction}
            disabled={busy}
            className="h-7 bg-background/60 px-2.5 text-xs transition-transform duration-200 hover:-translate-y-px"
          >
            {busy ? (
              <Loader2 data-icon="inline-start" className="animate-spin" />
            ) : status === "permission-denied" ? (
              <ExternalLink data-icon="inline-start" />
            ) : descriptor.actionLabel === "Dismiss" ? (
              <X data-icon="inline-start" />
            ) : (
              <RotateCcw data-icon="inline-start" />
            )}
            {busy ? "Working…" : descriptor.actionLabel}
          </Button>
        </div>
      )}
    </Alert>
  )
}
