export type MicStatus =
  | "ready"
  | "reconnecting"
  | "disconnected"
  | "permission-denied"
  | "no-devices"
  | "in-use"
  | "volume-unsupported"

export type StatusSeverity = "ok" | "info" | "warning" | "error"

export type StatusDescriptor = {
  severity: StatusSeverity
  // shown in the window status bar
  shortLabel: string
  // banner heading, null means no banner is shown for this status
  title: string | null
  description: string
  actionLabel?: string
  // mute toggle and device picker cant act on a missing or blocked endpoint
  controlsDisabled: boolean
  // windows exposes no volume endpoint for some virtual/aggregate devices
  volumeDisabled: boolean
}

export const MIC_STATUS: Record<MicStatus, StatusDescriptor> = {
  ready: {
    severity: "ok",
    shortLabel: "Device ready",
    title: null,
    description: "Microphone endpoint active.",
    controlsDisabled: false,
    volumeDisabled: false,
  },
  reconnecting: {
    severity: "info",
    shortLabel: "Reconnecting",
    title: "Reconnecting to device",
    description: "Waiting for Windows to report the endpoint. Mute state is preserved.",
    controlsDisabled: true,
    volumeDisabled: true,
  },
  disconnected: {
    severity: "error",
    shortLabel: "Device disconnected",
    title: "Microphone disconnected",
    description:
      "The selected device was unplugged. Mute state is held and will reapply when it returns.",
    actionLabel: "Retry",
    controlsDisabled: true,
    volumeDisabled: true,
  },
  "permission-denied": {
    severity: "error",
    shortLabel: "Access blocked",
    title: "Microphone access blocked",
    description:
      "Windows privacy settings deny app access to the microphone. Allow desktop apps, then retry.",
    actionLabel: "Open settings",
    controlsDisabled: true,
    volumeDisabled: true,
  },
  "no-devices": {
    severity: "warning",
    shortLabel: "No input devices",
    title: "No input devices found",
    description:
      "Windows reports no active capture endpoints. Connect a microphone or enable a disabled device.",
    actionLabel: "Rescan",
    controlsDisabled: true,
    volumeDisabled: true,
  },
  "in-use": {
    severity: "warning",
    shortLabel: "Exclusive mode",
    title: "Device held by another app",
    description:
      "Another app has exclusive control. Mute still applies at the driver level, but volume is locked.",
    actionLabel: "Dismiss",
    controlsDisabled: false,
    volumeDisabled: true,
  },
  "volume-unsupported": {
    severity: "info",
    shortLabel: "Volume unavailable",
    title: "Volume control unavailable",
    description:
      "This endpoint exposes no software volume. Mute works, but the level must be set on the device.",
    actionLabel: "Dismiss",
    controlsDisabled: false,
    volumeDisabled: true,
  },
}

export type HotkeyIssue = "none" | "conflict" | "needs-modifier" | "reserved"

export const HOTKEY_ISSUE: Record<HotkeyIssue, string | null> = {
  none: null,
  conflict: "Already registered by another app. Pick a different combination.",
  "needs-modifier": "Add Ctrl, Alt, or Shift so the key still works while typing.",
  reserved: "Reserved by Windows and cannot be captured.",
}

// combinations windows claims before any app can register them
const RESERVED = new Set(["Win+L", "Win+Tab", "Ctrl+Alt+Delete", "Win+G"])

// simulates a global RegisterHotKey collision with a known resident app
const TAKEN = new Set(["Ctrl+Shift+M", "Alt+F4", "Ctrl+Shift+Escape"])

export function validateChord(chord: string[]): HotkeyIssue {
  const combo = chord.join("+")
  if (RESERVED.has(combo)) return "reserved"
  if (TAKEN.has(combo)) return "conflict"

  const modifiers = chord.filter((key) => ["Ctrl", "Alt", "Shift", "Win"].includes(key))
  const bare = chord.filter((key) => !["Ctrl", "Alt", "Shift", "Win"].includes(key))
  const isTypingKey = bare.some((key) => key.length === 1 || key === "Space")

  if (modifiers.length === 0 && isTypingKey) return "needs-modifier"
  return "none"
}