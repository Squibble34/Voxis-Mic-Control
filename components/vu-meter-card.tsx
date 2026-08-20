"use client"

import * as React from "react"
import { Card, CardContent } from "@/components/ui/card"
import { cn } from "@/lib/utils"
import { AudioLinesIcon, type AudioLinesIconHandle } from "@/components/icons/audio-lines-icon"

const SEGMENT_COUNT = 12
const HOT_SEGMENT_START = 10

export function VuMeterCard({
  muted = false,
  disabled = false,
  level,
  reduceMotion = false,
}: {
  muted?: boolean
  disabled?: boolean
  level?: number
  reduceMotion?: boolean
}) {
  const [simulatedLevel, setSimulatedLevel] = React.useState(0)
  const [peakLevel, setPeakLevel] = React.useState(0)
  const peakDecayTimeout = React.useRef<ReturnType<typeof setTimeout> | null>(null)
  const audioLinesRef = React.useRef<AudioLinesIconHandle>(null)

  React.useEffect(() => {
    if (level !== undefined || muted || disabled) {
      setSimulatedLevel(0)
      return
    }
    let raf = 0
    let t = 0
    const animate = () => {
      t += 0.045
      const wobble = Math.sin(t) * 0.5 + Math.sin(t * 2.7) * 0.3 + Math.sin(t * 0.6) * 0.2
      setSimulatedLevel(Math.max(0, Math.min(100, 28 + wobble * 22)))
      raf = window.requestAnimationFrame(animate)
    }
    raf = window.requestAnimationFrame(animate)
    return () => window.cancelAnimationFrame(raf)
  }, [level, muted, disabled])

  const activeLevel = disabled || muted ? 0 : level ?? simulatedLevel
  const filledSegments = Math.round((activeLevel / 100) * SEGMENT_COUNT)

  React.useEffect(() => {
    setPeakLevel((prev) => (filledSegments > prev ? filledSegments : prev))
    if (peakDecayTimeout.current) clearTimeout(peakDecayTimeout.current)
    peakDecayTimeout.current = setTimeout(() => setPeakLevel(0), 800)
    return () => {
      if (peakDecayTimeout.current) clearTimeout(peakDecayTimeout.current)
    }
  }, [filledSegments])

  const statusLabel = disabled ? "--" : muted ? "Muted" : `${Math.round(activeLevel)}%`

  return (
    <Card
    onMouseEnter={() => { if (!reduceMotion) audioLinesRef.current?.startAnimation() }}
    onMouseLeave={() => audioLinesRef.current?.stopAnimation()}
      className={cn(
        "gap-0 py-0 transition-all duration-300",
        disabled
          ? "border-dashed border-border bg-card/60 opacity-90"
          : "border-border/70 bg-card/80 hover:border-border",
      )}
    >
      <CardContent className="flex flex-col gap-2 px-3 py-2.5">
        <div className="flex items-center gap-2">
          <span
            className={cn(
              "flex size-7 shrink-0 items-center justify-center rounded-lg border border-border/70 bg-muted/50 text-muted-foreground transition-colors duration-300",
              !disabled && !muted && activeLevel > 4 && "border-primary/40 bg-primary/10 text-primary",
            )}
          >
            <AudioLinesIcon ref={audioLinesRef} size={14} className="shrink-0" />
          </span>
          <span className="min-w-0 flex-1 truncate text-xs font-medium tracking-wide text-muted-foreground">
            Input level
          </span>
        </div>

        <div className="flex h-8 items-center justify-between gap-2">
          <div
            className="flex flex-1 items-end gap-[3px]"
            role="img"
            aria-label="Microphone input level"
          >
            {Array.from({ length: SEGMENT_COUNT }).map((_, index) => {
              const filled = index < filledSegments
              const isPeak = index === peakLevel - 1 && peakLevel > filledSegments
              const isHot = index >= HOT_SEGMENT_START
              return (
                <span
                  key={index}
                  className={cn(
                    "flex-1 rounded-[2px] transition-colors duration-150",
                    filled || isPeak ? (isHot ? "bg-warning" : "bg-primary") : "bg-muted/60",
                  )}
                  style={{ height: `${8 + index * 1.5}px` }}
                />
              )
            })}
          </div>
          <span
            className={cn(
              "inline-flex w-14 shrink-0 items-center justify-center rounded-md bg-muted/60 py-0.5 font-mono text-xs tabular-nums text-foreground/80",
              (muted || disabled) && "text-muted-foreground",
            )}
          >
            {statusLabel}
          </span>
        </div>
      </CardContent>
    </Card>
  )
}