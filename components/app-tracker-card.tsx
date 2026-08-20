"use client"

import * as React from "react"
import { AppWindow as AppWindowIcon, Mic } from "lucide-react"
import { Card, CardContent } from "@/components/ui/card"
import { cn } from "@/lib/utils"

export type TrackedApp = {
  id: string
  name: string
  active: boolean
  icon: string | null
}

const PLACEHOLDER_APPS: TrackedApp[] = []

export function AppTrackerCard({ apps = PLACEHOLDER_APPS }: { apps?: TrackedApp[] }) {
  return (
    <Card className="group/tracker gap-0 py-0 border-border/70 bg-card/80 backdrop-blur transition-colors duration-300 hover:border-border">
      <CardContent className="flex flex-col gap-2 px-3 py-2.5">
        <div className="flex items-center gap-2">
        <span className="flex size-7 shrink-0 items-center justify-center rounded-lg border border-border/70 bg-muted/50 text-muted-foreground">
            <Mic className="size-3.5 group-hover/tracker:animate-wiggle" />
          </span>
          <span className="min-w-0 flex-1 truncate text-xs font-medium tracking-wide text-muted-foreground">
            Mic Access
          </span>
          <span className="shrink-0 rounded-md bg-muted/60 px-2 py-0.5 text-xs font-medium tabular-nums text-foreground/80">
            {apps.length}
          </span>
        </div>

        {apps.length === 0 ? (
          <div className="flex items-center justify-center gap-1.5 py-1.5 text-center">
            <AppWindowIcon className="size-3.5 text-muted-foreground/60" />
            <span className="text-xs text-muted-foreground">
              No other apps are using your mic right now
            </span>
          </div>
        ) : (
          <div className="flex flex-wrap gap-1.5">
            {apps.map((app) => (
              <div
                key={app.id}
                className="group/app flex min-w-[150px] flex-1 items-center gap-2 rounded-md border border-border/60 bg-muted/30 px-2.5 py-1.5 transition-colors duration-200 hover:border-border hover:bg-muted/50"
              >
                <span className="relative flex size-7 shrink-0 items-center justify-center overflow-hidden rounded-md border border-border/70 bg-muted/50 text-muted-foreground">
                  {app.icon ? (
                    <img
                      src={app.icon}
                      alt=""
                      className="size-full object-contain transition-transform duration-300 group-hover/app:scale-110"
                    />
                  ) : (
                    <AppWindowIcon className="size-3.5 transition-transform duration-300 group-hover/app:scale-110" />
                  )}
                  {app.active && (
                    <span className="absolute -right-0.5 -top-0.5 size-1.5 animate-listening rounded-full bg-primary" />
                  )}
                </span>
                <span className="min-w-0 flex-1 truncate text-xs font-medium text-foreground">
                  {app.name}
                </span>
                <span
                  className={cn(
                    "shrink-0 text-[11px] font-medium",
                    app.active ? "text-primary" : "text-muted-foreground",
                  )}
                >
                  {app.active ? "Active" : "Idle"}
                </span>
              </div>
            ))}
          </div>
        )}
      </CardContent>
    </Card>
  )
}