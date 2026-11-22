"use client"

import { AlertTriangle, ExternalLink, Loader2, RotateCcw, ShieldAlert } from "lucide-react"

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

export type VirtualMicStatus = {
  state: "ready" | "installing" | "attaching" | "usbip-missing" | "install-failed" | "attach-failed"
  usbipInstalled: boolean
  virtualMicFound: boolean
  error: string | null
}

export function VirtualMicBanner({
  virtualMicStatus,
  onRetry,
  onOpenManualDownload,
}: {
  virtualMicStatus: VirtualMicStatus | null
  onRetry: (forceReinstall: boolean) => void
  onOpenManualDownload: () => void
}) {
  if (
    !virtualMicStatus ||
    virtualMicStatus.state === "ready" ||
    virtualMicStatus.state === "attaching"
  ) {
    return null
  }

  const isInstalling = virtualMicStatus.state === "installing"
  const isInstallError = virtualMicStatus.state === "install-failed"
  const isMissing = virtualMicStatus.state === "usbip-missing"
  const isAttachFailed = virtualMicStatus.state === "attach-failed"

  const Icon = isInstalling ? Loader2 : isInstallError ? ShieldAlert : AlertTriangle
  const tone = isInstalling
    ? "border-border/70 bg-muted/40 text-foreground"
    : isInstallError
      ? "border-destructive/45 bg-destructive/10 text-destructive"
      : "border-warning/45 bg-warning/10 text-foreground"

  const title = isInstalling
    ? "Installing virtual mic driver"
    : isInstallError
      ? "Virtual mic driver failed to install"
      : isMissing
        ? "Virtual mic driver missing"
        : "Virtual mic failed to attach"

  const description = isInstalling
    ? "Downloading and installing USBip so Windows and other apps can see Voxis Microphone. Approve the Windows UAC prompt if asked."
    : isAttachFailed
      ? "USBip is installed, but Voxis Microphone did not appear in Windows Sound Settings."
      : "Voxis Microphone won't appear in Windows Sound Settings or other apps until the USBip driver is installed."

  return (
    <Alert aria-live="polite" className={cn("animate-view-in items-start pr-2", tone)}>
      <Icon className={cn(isInstalling && "animate-spin")} />
      <AlertTitle className="text-[13px]">{title}</AlertTitle>
      <AlertDescription
        className={cn("text-xs leading-relaxed", isInstallError && "text-destructive/85")}
      >
        <p>{description}</p>
        {virtualMicStatus.error && (
          <p className="mt-1 break-all font-mono text-[11px] opacity-85">
            {virtualMicStatus.error}
          </p>
        )}
      </AlertDescription>
      {!isInstalling && (
        <div className="col-start-2 mt-1.5 flex flex-wrap items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            onClick={() => onRetry(!isAttachFailed)}
            className="h-7 bg-background/60 px-2.5 text-xs transition-transform duration-200 hover:-translate-y-px"
          >
            <RotateCcw data-icon="inline-start" />
            {isAttachFailed ? "Retry attach" : isMissing ? "Install driver" : "Retry install"}
          </Button>
          {isAttachFailed ? (
            <Button
              variant="outline"
              size="sm"
              onClick={() => onRetry(true)}
              className="h-7 bg-background/60 px-2.5 text-xs transition-transform duration-200 hover:-translate-y-px"
            >
              Reinstall driver
            </Button>
          ) : (
            <Button
              variant="outline"
              size="sm"
              onClick={onOpenManualDownload}
              className="h-7 bg-background/60 px-2.5 text-xs transition-transform duration-200 hover:-translate-y-px"
            >
              <ExternalLink data-icon="inline-start" />
              Download manually
            </Button>
          )}
        </div>
      )}
    </Alert>
  )
}
