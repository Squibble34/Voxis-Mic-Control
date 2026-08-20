"use client"

import { MIC_STATUS, type MicStatus } from "@/lib/mic-status"
import { cn } from "@/lib/utils"

const DOT_STYLES = {
  ok: "bg-primary",
  info: "bg-muted-foreground",
  warning: "bg-warning",
  error: "bg-destructive",
} as const

export function StatusBar({
  status,
  muted,
}: {
  status: MicStatus
  muted: boolean
}) {
  const descriptor = MIC_STATUS[status]

  return (
    <footer className="flex h-8 shrink-0 items-center gap-2 border-t border-border/70 bg-card/50 px-3">
      <span
        className={cn(
          "size-1.5 rounded-full transition-colors duration-300",
          DOT_STYLES[descriptor.severity],
          descriptor.severity === "ok" && !muted && "animate-listening",
        )}
      />
      <span className="truncate text-[11px] font-medium tracking-wide text-muted-foreground">
        {descriptor.shortLabel}
      </span>

      <span className="ml-auto font-mono text-[11px] tracking-wide text-muted-foreground/70">
        {muted ? "MUTED" : "OPEN"}
      </span>
    </footer>
  )
}
