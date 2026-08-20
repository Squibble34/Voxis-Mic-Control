"use client"

import * as React from "react"
import {
  Check,
  ChevronDown,
  Mic,
  MicOff,
  Unplug,
  Volume1,
  Volume2,
  VolumeX,
} from "lucide-react"

import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Card, CardContent, CardHeader } from "@/components/ui/card"
import { Slider } from "@/components/ui/slider"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { MIC_STATUS, type MicStatus } from "@/lib/mic-status"
import { cn } from "@/lib/utils"

export type Device = {
  id: string
  name: string
  detail: string
  available: boolean
}

export function MicController({
  status,
  devices,
  deviceId,
  onDeviceChange,
  muted,
  onMutedChange,
  volume,
  onVolumeChange,
}: {
  status: MicStatus
  devices: Device[]
  deviceId: string
  onDeviceChange: (id: string) => void
  muted: boolean
  onMutedChange: (muted: boolean) => void
  volume: number
  onVolumeChange: (volume: number) => void
}) {
  const [menuOpen, setMenuOpen] = React.useState(false)
  const [popKey, setPopKey] = React.useState(0)

  const descriptor = MIC_STATUS[status]
  const device = devices.find((d) => d.id === deviceId)
  const offline = descriptor.controlsDisabled
  const volumeLocked = offline || descriptor.volumeDisabled

  const VolumeIcon = muted || volume === 0 ? VolumeX : volume < 55 ? Volume1 : Volume2

  function toggleMute() {
    if (offline) return
    onMutedChange(!muted)
    setPopKey((k) => k + 1)
  }

  const statusLine = offline
    ? descriptor.shortLabel
    : muted
      ? "Muted"
      : "Live · Capturing audio"

  return (
    <Card
      data-offline={offline || undefined}
      className={cn(
        "gap-0 overflow-hidden border-border/70 bg-card/80 py-0 backdrop-blur transition-colors duration-300 hover:border-border",
        offline && "border-dashed opacity-90",
      )}
    >
      <CardHeader className="flex flex-row items-center gap-3 border-b border-border/60 px-3.5 py-3">
        <Tooltip>
          <TooltipTrigger render={<span className="shrink-0" />}>
            <button
              key={popKey}
              type="button"
              onClick={toggleMute}
              disabled={offline}
              aria-pressed={muted}
              aria-label={muted ? "Unmute microphone" : "Mute microphone"}
              className={cn(
                "group/mute relative flex size-12 animate-pop items-center justify-center rounded-full border transition-all duration-300 ease-out outline-none focus-visible:ring-3 focus-visible:ring-ring/50",
                offline
                  ? "cursor-not-allowed border-dashed border-border bg-muted/40 text-muted-foreground shadow-none"
                  : muted
                    ? "border-destructive/40 bg-destructive/15 text-destructive shadow-[0_0_24px_-8px_var(--destructive)] hover:bg-destructive/25"
                    : "animate-listening border-primary/40 bg-primary/12 text-primary hover:bg-primary/20",
              )}
            >
              {!offline && (
                <span
                  className={cn(
                    "absolute inset-0 rounded-full opacity-0 transition-opacity duration-500 group-hover/mute:opacity-100",
                    muted ? "bg-destructive/10" : "bg-primary/10",
                  )}
                />
              )}
              {offline ? (
                <Unplug className="relative size-5" />
              ) : muted ? (
                <MicOff className="relative size-5 transition-transform duration-300 ease-out group-hover/mute:scale-115 group-active/mute:scale-90" />
              ) : (
                <Mic className="relative size-5 transition-transform duration-300 ease-out group-hover/mute:scale-115 group-active/mute:scale-90" />
              )}
            </button>
          </TooltipTrigger>
          <TooltipContent side="bottom">
            {offline
              ? `Unavailable: ${descriptor.shortLabel.toLowerCase()}`
              : muted
                ? "Unmute microphone"
                : "Mute microphone"}
          </TooltipContent>
        </Tooltip>

        <div className="flex min-w-0 flex-1 flex-col items-start gap-0.5">
          <DropdownMenu open={menuOpen} onOpenChange={setMenuOpen}>
            <DropdownMenuTrigger
              disabled={status === "no-devices"}
              className="group/device -ml-1 flex max-w-full items-center gap-1.5 rounded-md px-1 py-0.5 text-left transition-colors duration-200 outline-none hover:bg-muted/60 focus-visible:ring-3 focus-visible:ring-ring/50 disabled:pointer-events-none disabled:opacity-60"
            >
              <span
                className={cn(
                  "truncate text-sm font-semibold tracking-tight",
                  device ? "text-foreground" : "text-muted-foreground italic",
                )}
              >
                {device?.name ?? "No device selected"}
              </span>
              <ChevronDown
                className={cn(
                  "size-3.5 shrink-0 text-muted-foreground transition-transform duration-300 ease-out",
                  menuOpen && "rotate-180 text-primary",
                )}
              />
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start" className="w-64">
              <DropdownMenuGroup>
                <DropdownMenuLabel className="text-[11px] tracking-wider uppercase">
                  Input devices
                </DropdownMenuLabel>
                <DropdownMenuSeparator />
                {devices.length === 0 ? (
                  <DropdownMenuItem disabled className="justify-center text-xs">
                    No capture endpoints
                  </DropdownMenuItem>
                ) : (
                  devices.map((item) => (
                    <DropdownMenuItem
                      key={item.id}
                      disabled={!item.available}
                      onClick={() => onDeviceChange(item.id)}
                      className="gap-2"
                    >
                      <span className="flex min-w-0 flex-col">
                        <span className="truncate">{item.name}</span>
                        <span className="truncate text-xs text-muted-foreground">
                          {item.available ? item.detail : "Unplugged"}
                        </span>
                      </span>
                      {item.id === deviceId && (
                        <Check className="ml-auto size-3.5 text-primary" />
                      )}
                    </DropdownMenuItem>
                  ))
                )}
              </DropdownMenuGroup>
            </DropdownMenuContent>
          </DropdownMenu>

          <span
            className={cn(
              "flex items-center gap-1.5 px-1 text-xs font-medium transition-colors duration-300",
              offline
                ? "text-muted-foreground"
                : muted
                  ? "text-destructive"
                  : "text-muted-foreground",
            )}
          >
            {statusLine}
            {muted && offline && (
              <span className="rounded-sm border border-border bg-muted/60 px-1 py-px text-[10px] tracking-wide text-muted-foreground">
                MUTE HELD
              </span>
            )}
          </span>
        </div>
      </CardHeader>

      <CardContent className="group/slider flex flex-col gap-2 px-3.5 py-3">
        <div className="flex items-center justify-between">
          <span className="flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
            <VolumeIcon className="size-3.5" />
            Input volume
          </span>
          <span
            className={cn(
              "font-mono text-xs tabular-nums",
              volumeLocked ? "text-muted-foreground/60" : "text-foreground/80",
            )}
          >
            {volumeLocked ? "--" : `${volume}%`}
          </span>
        </div>
        <Slider
          value={volumeLocked ? 0 : volume}
          onValueChange={(value) => onVolumeChange(value as number)}
          disabled={volumeLocked}
          max={100}
          step={1}
          aria-label="Input volume"
          className={cn(
            "[&_[data-slot=slider-thumb]]:border-primary [&_[data-slot=slider-thumb]]:bg-primary [&_[data-slot=slider-thumb]]:transition-transform [&_[data-slot=slider-thumb]]:duration-300 [&_[data-slot=slider-thumb]]:ease-out",
            "[&_[data-slot=slider-track]]:h-1.5 [&_[data-slot=slider-track]]:bg-muted/80",
            !volumeLocked && "group-hover/slider:[&_[data-slot=slider-thumb]]:scale-140",
            volumeLocked &&
              "opacity-70 [&_[data-slot=slider-thumb]]:border-border [&_[data-slot=slider-thumb]]:bg-muted-foreground/40 [&_[data-slot=slider-track]]:bg-muted/50",
            !volumeLocked &&
              muted &&
              "opacity-60 [&_[data-slot=slider-range]]:bg-destructive [&_[data-slot=slider-thumb]]:border-destructive [&_[data-slot=slider-thumb]]:bg-destructive",
          )}
        />
      </CardContent>
    </Card>
  )
}
