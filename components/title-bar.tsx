"use client"

import { Mic, Minus, Settings, X } from "lucide-react"
import { getCurrentWindow } from "@tauri-apps/api/window"

import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

export type AppView = "main" | "settings"

interface TitleBarProps {
  activeView: AppView
  onViewChange: (view: AppView) => void
}

export function TitleBar({ activeView, onViewChange }: TitleBarProps) {
  return (
    <header
      data-tauri-drag-region
      className="flex h-[38px] shrink-0 items-center justify-between border-b border-border/70 bg-card px-1.5 select-none"
    >
      <div className="flex items-center gap-0.5">
        <NavButton
          label="Settings"
          active={activeView === "settings"}
          onClick={() => onViewChange("settings")}
        >
          <Settings
            className={cn(
              "transition-transform duration-500 ease-out",
              activeView === "settings" ? "rotate-90" : "group-hover/nav:rotate-45",
            )}
          />
        </NavButton>
        <NavButton
          label="Home"
          active={activeView === "main"}
          onClick={() => onViewChange("main")}
        >
          <Mic className="transition-transform duration-300 ease-out group-hover/nav:-translate-y-0.5" />
        </NavButton>
      </div>

      <div
        data-tauri-drag-region
        className="pointer-events-none flex-1 px-3 text-center text-[11px] font-medium tracking-[0.14em] text-muted-foreground/70 uppercase"
      >
        Voxis Mic Control
      </div>

      <div className="flex items-center gap-0.5">
        <WindowButton label="Minimize" onClick={() => getCurrentWindow().minimize()}>
          <Minus />
        </WindowButton>
        <WindowButton label="Close" destructive onClick={() => getCurrentWindow().close()}>
          <X />
        </WindowButton>
      </div>
    </header>
  )
}

function NavButton({
  label,
  active,
  onClick,
  children,
}: {
  label: string
  active: boolean
  onClick: () => void
  children: React.ReactNode
}) {
  return (
    <Button
      variant="ghost"
      size="icon-sm"
      aria-label={label}
      aria-pressed={active}
      onClick={onClick}
      className={cn(
        "group/nav rounded-full text-muted-foreground transition-all duration-300 hover:scale-105 hover:bg-muted/70 hover:text-foreground active:scale-95",
        active && "bg-primary/12 text-primary hover:bg-primary/18 hover:text-primary",
      )}
    >
      {children}
    </Button>
  )
}

function WindowButton({
  label,
  destructive,
  onClick,
  children,
}: {
  label: string
  destructive?: boolean
  onClick?: () => void
  children: React.ReactNode
}) {
  return (
    <Button
      variant="ghost"
      size="icon-sm"
      aria-label={label}
      onClick={onClick}
      className={cn(
        "rounded-md text-muted-foreground transition-colors duration-200 hover:text-foreground",
        destructive && "hover:bg-destructive/15 hover:text-destructive",
      )}
    >
      {children}
    </Button>
  )
}
