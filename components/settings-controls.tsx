"use client"

import * as React from "react"
import { Pencil, RotateCcw, X } from "lucide-react"
import type { ChangelogEntry } from "@/lib/changelog"

import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Slider } from "@/components/ui/slider"
import { Switch } from "@/components/ui/switch"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

// splits on backtick pairs so changelog items can write `like this` for inline code
function renderInlineText(text: string) {
  const parts = text.split(/(`[^`]+`)/g)
  return parts.map((part, index) =>
    part.startsWith("`") && part.endsWith("`") ? (
      <code key={index} className="rounded bg-muted/60 px-1 py-px font-mono text-[11px]">
        {part.slice(1, -1)}
      </code>
    ) : (
      part
    ),
  )
}

export function SettingsSection({
  title,
  description,
  children,
}: {
  title: string
  description: string
  children: React.ReactNode
}) {
  return (
    <section className="flex flex-col gap-3">
      <div className="flex flex-col gap-0.5">
        <h2 className="text-sm font-semibold tracking-tight text-foreground">{title}</h2>
        <p className="text-xs leading-relaxed text-muted-foreground">{description}</p>
      </div>
      <div className="flex flex-col gap-3">{children}</div>
    </section>
  )
}

export function SettingsGroup({
  title,
  description,
  children,
}: {
  title?: string
  description?: string
  children: React.ReactNode
}) {
  return (
    <div className="flex flex-col gap-2.5 rounded-lg border border-border/50 bg-background/40 p-3">
      {(title || description) && (
        <div className="flex flex-col gap-0.5">
          {title && (
            <span className="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground/80">
              {title}
            </span>
          )}
          {description && (
            <span className="text-xs leading-relaxed text-muted-foreground">{description}</span>
          )}
        </div>
      )}
      <div className="flex flex-col gap-3">{children}</div>
    </div>
  )
}

export function ToggleRow({
  label,
  labelSuffix,
  hint,
  checked,
  defaultChecked,
  onCheckedChange,
  disabled,
}: {
  label: string
  labelSuffix?: React.ReactNode
  hint: string
  checked?: boolean
  defaultChecked?: boolean
  onCheckedChange?: (checked: boolean) => void
  disabled?: boolean
}) {
  const id = React.useId()
  return (
    <div className={cn("group/row flex items-center justify-between gap-4", disabled && "opacity-50")}>
      <div className="flex min-w-0 flex-col gap-0.5">
        <label
          htmlFor={id}
          className="text-[13px] font-medium text-foreground transition-colors duration-200 group-hover/row:text-primary"
        >
          {label}
          {labelSuffix}
        </label>
        <span className="text-xs leading-relaxed text-muted-foreground">{hint}</span>
      </div>
      <Switch
        id={id}
        checked={checked}
        defaultChecked={defaultChecked}
        onCheckedChange={onCheckedChange}
        disabled={disabled}
      />
    </div>
  )
}

export function SelectRow({
  label,
  hint,
  value,
  onValueChange,
  options,
  disabled,
}: {
  label: string
  hint: string
  value: string
  onValueChange?: (value: string) => void
  options: { value: string; label: string }[]
  disabled?: boolean
}) {
  return (
    <div className={cn("group/row flex flex-col gap-2", disabled && "opacity-50")}>
      <div className="flex min-w-0 flex-col gap-0.5">
        <span className="text-[13px] font-medium text-foreground transition-colors duration-200 group-hover/row:text-primary">
          {label}
        </span>
        <span className="text-xs leading-relaxed text-muted-foreground">{hint}</span>
      </div>
      <Select items={options} value={value} onValueChange={(next) => onValueChange?.(next as string)} disabled={disabled}>
        <SelectTrigger size="sm" className="w-full">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectGroup>
            {options.map((option) => (
              <SelectItem key={option.value} value={option.value}>
                {option.label}
              </SelectItem>
            ))}
          </SelectGroup>
        </SelectContent>
      </Select>
    </div>
  )
}

// same sliding highlight motion as the section nav rail
export function SegmentedRow({
  label,
  hint,
  value,
  onValueChange,
  options,
  disabled,
}: {
  label: string
  hint: string
  value: string
  onValueChange?: (value: string) => void
  options: { value: string; label: string }[]
  disabled?: boolean
}) {
  const activeIndex = Math.max(0, options.findIndex((option) => option.value === value))

  return (
    <div className={cn("group/row flex flex-col gap-2", disabled && "opacity-50")}>
      <div className="flex min-w-0 flex-col gap-0.5">
        <span className="text-[13px] font-medium text-foreground transition-colors duration-200 group-hover/row:text-primary">
          {label}
        </span>
        <span className="text-xs leading-relaxed text-muted-foreground">{hint}</span>
      </div>
      <div className="relative flex overflow-hidden rounded-md border border-border/60 bg-muted/40 p-1">
        <span
          aria-hidden
          className="absolute inset-y-1 left-1 rounded-sm bg-primary/12 ring-1 ring-primary/25 transition-transform duration-300 ease-[cubic-bezier(0.22,1,0.36,1)]"
          style={{
            width: `calc((100% - 0.5rem) / ${options.length})`,
            transform: `translateX(calc(${activeIndex} * 100%))`,
          }}
        />
        {options.map((option) => {
          const isActive = option.value === value
          return (
            <button
              key={option.value}
              type="button"
              disabled={disabled}
              onClick={() => onValueChange?.(option.value)}
              aria-pressed={isActive}
              className={cn(
                "relative z-10 flex-1 rounded-sm px-2 py-1 text-[12px] font-medium transition-colors duration-200 outline-none focus-visible:ring-3 focus-visible:ring-ring/50",
                isActive ? "text-primary" : "text-muted-foreground hover:text-foreground",
                disabled && "cursor-not-allowed",
              )}
            >
              {option.label}
            </button>
          )
        })}
      </div>
    </div>
  )
}

export function SliderRow({
  label,
  hint,
  value,
  onValueChange,
  min = 0,
  max = 100,
  step = 1,
  unit = "",
  disabled,
}: {
  label: string
  hint: string
  value: number
  onValueChange?: (value: number) => void
  min?: number
  max?: number
  step?: number
  unit?: string
  disabled?: boolean
}) {
  return (
    <div className={cn("group/row flex flex-col gap-2.5", disabled && "opacity-50")}>
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 flex-col gap-0.5">
          <span className="text-[13px] font-medium text-foreground transition-colors duration-200 group-hover/row:text-primary">
            {label}
          </span>
          <span className="text-xs leading-relaxed text-muted-foreground">{hint}</span>
        </div>
        <span className="shrink-0 rounded-md bg-muted/60 px-2 py-0.5 text-xs font-medium tabular-nums text-foreground">
          {value}
          {unit}
        </span>
      </div>
      <Slider
        value={value}
        min={min}
        max={max}
        step={step}
        disabled={disabled}
        onValueChange={(next) => onValueChange?.(Array.isArray(next) ? next[0] : next)}
      />
    </div>
  )
}

export function AccentColorRow({
  color,
  themeDefaultHex,
  onColorChange,
}: {
  // null means use the theme's built in default instead of a custom accent
  color: string | null
  // the themes actual default, shown in the swatch when color is null
  themeDefaultHex: string
  onColorChange?: (color: string | null) => void
}) {
  const isDefault = color === null
  const displayColor = color ?? themeDefaultHex

  return (
    <div className="group/row flex items-center justify-between gap-4">
      <div className="flex min-w-0 flex-col gap-0.5">
        <span className="text-[13px] font-medium text-foreground transition-colors duration-200 group-hover/row:text-primary">
          Accent color
        </span>
        <span className="text-xs leading-relaxed text-muted-foreground">
          Overrides the app's accent everywhere it's used.
        </span>
      </div>
      <div className="flex items-center gap-2">
        <label
          className="relative flex size-7 shrink-0 cursor-pointer items-center justify-center rounded-md border border-border/70 shadow-[0_1px_0_0_var(--border)] transition-transform duration-200 hover:-translate-y-px"
          style={{ backgroundColor: displayColor }}
        >
          <input
            type="color"
            value={displayColor}
            onChange={(event) => onColorChange?.(event.target.value)}
            className="absolute inset-0 size-full cursor-pointer opacity-0"
            aria-label="Accent color"
          />
        </label>
        <span className="w-[6.5ch] font-mono text-xs uppercase text-muted-foreground">
          {isDefault ? "Auto" : displayColor}
        </span>
        <Button
          variant="outline"
          size="sm"
          disabled={isDefault}
          onClick={() => onColorChange?.(null)}
          className={cn("size-7 p-0 text-destructive", isDefault && "opacity-40")}
          aria-label="Reset accent color"
        >
          <RotateCcw className="size-3.5" />
        </Button>
      </div>
    </div>
  )
}

// click once to arm, click again to confirm, arms back down on its own after a few seconds
export function ResetAllRow({ onConfirm }: { onConfirm?: () => void }) {
  const [confirming, setConfirming] = React.useState(false)

  React.useEffect(() => {
    if (!confirming) return
    const timeout = window.setTimeout(() => setConfirming(false), 4000)
    return () => window.clearTimeout(timeout)
  }, [confirming])

  return (
    <div className="flex items-center justify-between gap-4">
      <div className="flex min-w-0 flex-col gap-0.5">
        <span className="text-[13px] font-medium text-foreground">Reset all settings</span>
        <span className="text-xs leading-relaxed text-muted-foreground">
          Restores every setting on this page to its default value. This can't be undone.
        </span>
      </div>
      <div className="flex items-center gap-2">
        {confirming && (
          <Button variant="outline" size="sm" onClick={() => setConfirming(false)}>
            Cancel
          </Button>
        )}
        <Button
          variant="destructive"
          size="sm"
          onClick={() => {
            if (confirming) {
              onConfirm?.()
              setConfirming(false)
            } else {
              setConfirming(true)
            }
          }}
          className="transition-transform duration-200 hover:-translate-y-px"
        >
          {confirming ? "Confirm reset" : "Reset all settings"}
        </Button>
      </div>
    </div>
  )
}

const MODIFIERS = ["Control", "Alt", "Shift", "Meta"]


export function HotkeyRow({
  label,
  hint,
  chord,
  onChordChange,
  disabled,
}: {
  label: string
  hint: string
  chord: string[]
  onChordChange?: (chord: string[]) => void
  disabled?: boolean
}) {
  const [listening, setListening] = React.useState(false)

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
      onChordChange?.(parts)
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

  return (
    <div className={cn("group/row flex items-center justify-between gap-4", disabled && "opacity-50")}>
      <div className="flex min-w-0 flex-col gap-0.5">
        <span className="text-[13px] font-medium text-foreground transition-colors duration-200 group-hover/row:text-primary">
          {label}
        </span>
        <span className="text-xs leading-relaxed text-muted-foreground">{hint}</span>
      </div>

      <div className="flex items-center gap-2">
        {listening ? (
          <span className="flex items-center gap-2 text-xs font-medium text-primary">
            <span className="size-1.5 animate-listening rounded-full bg-primary" />
            Press a key…
          </span>
        ) : (
          <span className="flex flex-wrap items-center gap-1">
            {chord.map((key, index) => (
              <React.Fragment key={key + index}>
                {index > 0 && <span className="text-xs text-muted-foreground/60">+</span>}
                <kbd className="rounded-md border border-border bg-muted/60 px-1.5 py-0.5 font-mono text-[11px] font-medium text-foreground shadow-[0_1px_0_0_var(--border)]">
                  {key}
                </kbd>
              </React.Fragment>
            ))}
          </span>
        )}

        <Button
          variant={listening ? "destructive" : "outline"}
          size="sm"
          disabled={disabled}
          onClick={() => setListening((prev) => !prev)}
          className="transition-transform duration-200 hover:-translate-y-px"
        >
          {listening ? (
            <>
              <X data-icon="inline-start" />
              Cancel
            </>
          ) : (
            <>
              <Pencil data-icon="inline-start" />
              Edit
            </>
          )}
        </Button>
      </div>
    </div>
  )
}

export function ActionRow({
  label,
  hint,
  buttonLabel,
  onAction,
  disabled,
}: {
  label: string
  hint: string
  buttonLabel: string
  onAction?: () => void
  disabled?: boolean
}) {
  return (
    <div className={cn("group/row flex items-center justify-between gap-4", disabled && "opacity-50")}>
      <div className="flex min-w-0 flex-col gap-0.5">
        <span className="text-[13px] font-medium text-foreground transition-colors duration-200 group-hover/row:text-primary">
          {label}
        </span>
        <span className="text-xs leading-relaxed text-muted-foreground">{hint}</span>
      </div>
      <Button
        variant="outline"
        size="sm"
        disabled={disabled}
        onClick={onAction}
        className="shrink-0 transition-transform duration-200 hover:-translate-y-px"
      >
        {buttonLabel}
      </Button>
    </div>
  )
}

function ChangelogEntryBlock({ entry }: { entry: ChangelogEntry }) {
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-baseline gap-2">
        <span className="text-[13px] font-medium text-foreground">v{entry.version}</span>
        <span className="text-xs text-muted-foreground">{entry.date}</span>
      </div>
      {entry.sections.map((section) => (
        <div key={section.label} className="flex flex-col gap-0.5">
          <span className="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground/80">
            {section.label}
          </span>
          <ul className="flex flex-col gap-0.5">
            {section.items.map((item, index) => (
              <li key={index} className="text-xs leading-relaxed text-muted-foreground">
                • {renderInlineText(item)}
              </li>
            ))}
          </ul>
        </div>
      ))}
    </div>
  )
}

export function ChangelogList({ entries }: { entries: ChangelogEntry[] }) {
  const [expanded, setExpanded] = React.useState(false)
  const [latest, ...rest] = entries

  return (
    <div className="flex flex-col gap-3">
      {expanded ? (
        entries.map((entry) => <ChangelogEntryBlock key={entry.version} entry={entry} />)
      ) : (
        <div
          className="relative max-h-28 overflow-hidden"
          style={{
            WebkitMaskImage: "linear-gradient(to bottom, black 65%, transparent 100%)",
            maskImage: "linear-gradient(to bottom, black 65%, transparent 100%)",
          }}
        >
          <ChangelogEntryBlock entry={latest} />
        </div>
      )}
      {rest.length > 0 && (
        <Button variant="outline" size="sm" onClick={() => setExpanded((prev) => !prev)}>
          {expanded ? "Show less" : `Show all (${rest.length} more version${rest.length === 1 ? "" : "s"})`}
        </Button>
      )}
    </div>
  )
}