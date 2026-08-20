"use client"

import * as React from "react"
import { AlertTriangle, Keyboard, Pencil, X } from "lucide-react"

import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { HOTKEY_ISSUE, validateChord, type HotkeyIssue } from "@/lib/mic-status"
import { cn } from "@/lib/utils"

const MODIFIERS = ["Control", "Alt", "Shift", "Meta"]

export function HotkeyCard({
  disabled = false,
  chord,
  onChordChange,
  registerError = false,
  mode,
  onModeChange,
}: {
  disabled?: boolean
  chord: string[]
  onChordChange: (chord: string[]) => void
  registerError?: boolean
  mode: "toggle" | "push-to-talk"
  onModeChange: (mode: "toggle" | "push-to-talk") => void
}) {
  const [listening, setListening] = React.useState(false)
  const [rejection, setRejection] = React.useState<HotkeyIssue | null>(null)
  const issue = React.useMemo(() => validateChord(chord), [chord])

  React.useEffect(() => {
    if (listening) setRejection(null)
  }, [listening])

  React.useEffect(() => {
    if (!listening) return

    function handleKeyDown(event: KeyboardEvent) {
      event.preventDefault()
      if (event.key === "Escape") {
        setListening(false)
        return
      }

      const parts: string[] = []
      if (event.ctrlKey) parts.push("Ctrl")
      if (event.altKey) parts.push("Alt")
      if (event.shiftKey) parts.push("Shift")
      if (event.metaKey) parts.push("Super")

      const key = event.key
      if (MODIFIERS.includes(key)) return

      parts.push(key === " " ? "Space" : key.length === 1 ? key.toUpperCase() : key)
      const nextIssue = validateChord(parts)

      // reserved chords never reach an app, so keep the old binding instead of overwriting it with a dead one
      if (nextIssue === "reserved") {
        setRejection("reserved")
        setListening(false)
        return
      }

      onChordChange(parts)
      setListening(false)
    }

    function handleBlur() {
      setListening(false)
    }

    window.addEventListener("keydown", handleKeyDown)
    window.addEventListener("blur", handleBlur)
    return () => {
      window.removeEventListener("keydown", handleKeyDown)
      window.removeEventListener("blur", handleBlur)
    }
  }, [listening, onChordChange])

  React.useEffect(() => {
    if (disabled) setListening(false)
  }, [disabled])

  const issueMessage = HOTKEY_ISSUE[rejection ?? issue]
  const broken = issue !== "none" || registerError

  return (
    <Card
      className={cn(
        "group/hotkey gap-0 py-0 transition-all duration-300",
        listening
          ? "border-primary/60 bg-primary/5"
          : broken
            ? "border-warning/50 bg-warning/5"
            : "border-border/70 bg-card/80 hover:border-border",
      )}
    >
      <CardContent className="flex flex-col gap-2 px-3 py-2.5">
        <div className="flex items-center gap-2">
          <span
            className={cn(
              "flex size-7 shrink-0 items-center justify-center rounded-lg border border-border/70 bg-muted/50 text-muted-foreground transition-colors duration-300",
              listening && "border-primary/40 bg-primary/10 text-primary",
              !listening && broken && "border-warning/40 bg-warning/10 text-warning",
            )}
          >
            <Keyboard
              className={cn(
                "size-3.5",
                listening ? "animate-bob" : "group-hover/hotkey:animate-bob",
              )}
            />
          </span>
          <span className="min-w-0 flex-1 truncate text-xs font-medium tracking-wide text-muted-foreground">
            Hotkey
          </span>
          <Select
            items={[
              { value: "toggle", label: "Toggle" },
              { value: "push-to-talk", label: "Push to talk" },
            ]}
            value={mode}
            onValueChange={(value) => onModeChange(value as "toggle" | "push-to-talk")}
            disabled={disabled || listening}
          >
            <SelectTrigger size="sm" className="h-6 w-auto shrink-0 gap-1 px-2 text-[11px]">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                <SelectItem value="toggle">Toggle</SelectItem>
                <SelectItem value="push-to-talk">Push to talk</SelectItem>
              </SelectGroup>
            </SelectContent>
          </Select>
        </div>

        <div className="flex h-8 items-center justify-between gap-2">
          {listening ? (
            <span className="flex items-center gap-2 text-sm font-medium text-primary">
              <span className="size-1.5 animate-listening rounded-full bg-primary" />
              Press keys…
            </span>
          ) : (
            <span className="flex min-w-0 flex-wrap items-center gap-1">
              {chord.map((key, index) => (
                <React.Fragment key={key + index}>
                  {index > 0 && (
                    <span className="text-xs text-muted-foreground/60">+</span>
                  )}
                  <kbd
                    className={cn(
                      "rounded-md border bg-muted/60 px-1.5 py-0.5 font-mono text-[11px] font-medium shadow-[0_1px_0_0_var(--border)] transition-colors duration-300",
                      broken
                        ? "border-warning/50 text-warning line-through decoration-warning/60"
                        : "border-border text-foreground",
                      disabled && "opacity-60",
                    )}
                  >
                    {key}
                  </kbd>
                </React.Fragment>
              ))}
            </span>
          )}

          <Button
            variant={listening ? "destructive" : broken ? "default" : "outline"}
            size="sm"
            disabled={disabled}
            onClick={() => setListening((prev) => !prev)}
            className="shrink-0 transition-transform duration-200 hover:-translate-y-px"
          >
            {listening ? (
              <>
                <X data-icon="inline-start" />
                Cancel
              </>
            ) : (
              <>
                <Pencil data-icon="inline-start" />
                {broken ? "Rebind" : "Edit"}
              </>
            )}
          </Button>
        </div>

        {!listening && issueMessage && (
          <p className="animate-view-in flex items-start gap-1.5 text-xs leading-relaxed text-warning">
            <AlertTriangle className="mt-px size-3.5 shrink-0" />
            {issueMessage}
          </p>
        )}

        {!listening && !issueMessage && registerError && (
          <p className="animate-view-in flex items-start gap-1.5 text-xs leading-relaxed text-warning">
            <AlertTriangle className="mt-px size-3.5 shrink-0" />
            Windows rejected this shortcut.
          </p>
        )}

        {!listening && !issueMessage && disabled && (
          <p className="text-xs leading-relaxed text-muted-foreground">
            Nothing to toggle until a device is available.
          </p>
        )}
      </CardContent>
    </Card>
  )
}