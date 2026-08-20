"use client"

import * as React from "react"
import { AppWindow, Info, Keyboard, Palette, ShieldAlert, Sparkles } from "lucide-react"
import { getVersion } from "@tauri-apps/api/app"
import { CHANGELOG } from "@/lib/changelog"
import { SlidersHorizontalIcon, type SlidersHorizontalIconHandle } from "@/components/icons/sliders-horizontal-icon"
import {
  DEFAULT_APPEARANCE_SETTINGS,
  DEFAULT_KEYBINDS_SETTINGS,
  DEFAULT_SYSTEM_SETTINGS,
  DEFAULT_WINDOWS_SETTINGS,
  THEME_PRIMARY_HEX,
  type AppearanceSettings,
  type KeybindsSettings,
  type SystemSettings,
  type WindowsSettings,
} from "@/lib/settings-types"

import { Separator } from "@/components/ui/separator"
import { cn } from "@/lib/utils"
import {
  AccentColorRow,
  ActionRow,
  ChangelogList,
  HotkeyRow,
  ResetAllRow,
  SegmentedRow,
  SelectRow,
  SettingsGroup,
  SettingsSection,
  SliderRow,
  ToggleRow,
} from "@/components/settings-controls"

type SectionId = "general" | "windows" | "behavior" | "appearance" | "keybinds" | "danger"

const SECTIONS: { id: SectionId; label: string; icon: React.ElementType | null }[] = [
  { id: "general", label: "General", icon: Info },
  { id: "windows", label: "Windows", icon: AppWindow },
  { id: "behavior", label: "Behavior", icon: null },
  { id: "appearance", label: "Appearance", icon: Palette },
  { id: "keybinds", label: "Keybinds", icon: Keyboard },
  { id: "danger", label: "Danger Zone", icon: ShieldAlert },
]

const ICON_HOVER_ANIMATION: Record<SectionId, string> = {
  general: "group-hover:animate-pop",
  windows: "group-hover:animate-wiggle",
  behavior: "group-hover:animate-heartbeat",
  appearance: "group-hover:animate-hue-cycle",
  keybinds: "group-hover:animate-bob",
  danger: "group-hover:animate-shake",
}

const STARTUP_STATE_OPTIONS = [
  { value: "tray", label: "Hidden in tray" },
  { value: "visible", label: "Visible" },
  { value: "minimized", label: "Visible, minimized" },
]

const THEME_OPTIONS = [
  { value: "dark", label: "Dark" },
  { value: "light", label: "Light" },
]

const POPUP_LOCATION_OPTIONS = [
  { value: "top-left", label: "Top left" },
  { value: "top-right", label: "Top right" },
  { value: "bottom-right", label: "Bottom right" },
  { value: "bottom-left", label: "Bottom left" },
]

const FUN_MODE_CLICKS = 3
const FUN_MODE_WINDOW_MS = 600
const GITHUB_REPO = "Squibble34/Voxis-Mic-Control"

export function SettingsPanel({
  systemSettings,
  onSystemSettingsChange,
  appearanceSettings,
  onAppearanceSettingsChange,
  keybindsSettings,
  onKeybindsSettingsChange,
  windowsSettings,
  onWindowsSettingsChange,
  funModeUnlocked,
  onFunModeUnlockedChange,
  funModeEnabled,
  onFunModeEnabledChange,
  crazyColors,
  onCrazyColorsChange,
  randomMute,
  onRandomMuteChange,
  normalVolume,
  onNormalVolumeChange,
}: {
  systemSettings: SystemSettings
  onSystemSettingsChange: (patch: Partial<SystemSettings>) => void
  appearanceSettings: AppearanceSettings
  onAppearanceSettingsChange: (patch: Partial<AppearanceSettings>) => void
  keybindsSettings: KeybindsSettings
  onKeybindsSettingsChange: (patch: Partial<KeybindsSettings>) => void
  windowsSettings: WindowsSettings
  onWindowsSettingsChange: (patch: Partial<WindowsSettings>) => void
  funModeUnlocked: boolean
  onFunModeUnlockedChange: (unlocked: boolean) => void
  funModeEnabled: boolean
  onFunModeEnabledChange: (enabled: boolean) => void
  crazyColors: boolean
  onCrazyColorsChange: (enabled: boolean) => void
  randomMute: boolean
  onRandomMuteChange: (enabled: boolean) => void
  normalVolume: boolean
  onNormalVolumeChange: (enabled: boolean) => void
}) {
  const [active, setActive] = React.useState<SectionId>("general")
  const activeIndex = SECTIONS.findIndex((section) => section.id === active)
  const slidersRef = React.useRef<SlidersHorizontalIconHandle>(null)
  const [appVersion, setAppVersion] = React.useState("")
  const [updateStatus, setUpdateStatus] = React.useState<"idle" | "checking" | "current" | "available" | "error">("idle")
  const [latestVersion, setLatestVersion] = React.useState<string | null>(null)

  React.useEffect(() => {
    getVersion().then(setAppVersion).catch(console.error)
  }, [])

  function handleCheckForUpdate() {
    setUpdateStatus("checking")
    fetch(`https://api.github.com/repos/${GITHUB_REPO}/releases/latest`)
      .then((res) => {
        if (!res.ok) throw new Error(`GitHub API returned ${res.status}`)
        return res.json()
      })
      .then((release: { tag_name: string }) => {
        const tag = release.tag_name.replace(/^v/, "")
        setLatestVersion(tag)
        setUpdateStatus(tag === appVersion ? "current" : "available")
      })
      .catch((err) => {
        console.error("Update check failed:", err)
        setUpdateStatus("error")
      })
  }

  const scrollRef = React.useRef<HTMLDivElement | null>(null)
  const [showFade, setShowFade] = React.useState(false)

  const checkFade = React.useCallback(() => {
    const el = scrollRef.current
    if (!el) return
    const overflowing = el.scrollHeight > el.clientHeight + 1
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight <= 1
    setShowFade(overflowing && !atBottom)
  }, [])

  React.useEffect(() => {
    checkFade()
    const el = scrollRef.current
    if (!el) return
    const observer = new ResizeObserver(checkFade)
    observer.observe(el)
    return () => observer.disconnect()
  }, [active, checkFade])

  const dangerClickTimes = React.useRef<number[]>([])

  function handleNavClick(id: SectionId) {
    setActive(id)
    if (id !== "danger" || funModeUnlocked) return

    const now = Date.now()
    dangerClickTimes.current = [...dangerClickTimes.current, now].filter(
      (t) => now - t < FUN_MODE_WINDOW_MS,
    )
    if (dangerClickTimes.current.length >= FUN_MODE_CLICKS) {
      onFunModeUnlockedChange(true)
      dangerClickTimes.current = []
    }
  }

  function handleResetAll() {
    onWindowsSettingsChange(DEFAULT_WINDOWS_SETTINGS)
    onSystemSettingsChange(DEFAULT_SYSTEM_SETTINGS)
    onAppearanceSettingsChange(DEFAULT_APPEARANCE_SETTINGS)
    onKeybindsSettingsChange(DEFAULT_KEYBINDS_SETTINGS)
    onFunModeEnabledChange(false)
    onCrazyColorsChange(false)
    onRandomMuteChange(false)
    onNormalVolumeChange(false)
  }

  return (
    <div className="flex h-full min-h-0 gap-3">
      <nav
        aria-label="Settings sections"
        className="relative w-40 shrink-0 rounded-lg border border-border/60 bg-card/50 p-1.5"
      >
        <span
          aria-hidden
          className="absolute inset-x-1.5 h-8 rounded-md bg-primary/12 ring-1 ring-primary/25 transition-transform duration-300 ease-[cubic-bezier(0.22,1,0.36,1)]"
          style={{ transform: `translateY(${activeIndex * 36}px)` }}
        />
        <ul className="relative flex flex-col gap-1">
        {SECTIONS.map((section) => {
            const Icon = section.icon
            const isActive = section.id === active
            const isBehavior = section.id === "behavior"
            return (
              <li key={section.id}>
                <button
                  type="button"
                  onClick={() => handleNavClick(section.id)}
                  onMouseEnter={isBehavior ? () => { if (!appearanceSettings.reduceMotion) slidersRef.current?.startAnimation() } : undefined}
                  onMouseLeave={isBehavior ? () => slidersRef.current?.stopAnimation() : undefined}
                  aria-current={isActive ? "page" : undefined}
                  className={cn(
                    "group flex h-8 w-full items-center gap-2 rounded-md px-2.5 text-left text-[13px] font-medium transition-colors duration-200 outline-none focus-visible:ring-3 focus-visible:ring-ring/50",
                    isActive
                      ? "text-primary"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  {isBehavior ? (
                    <SlidersHorizontalIcon ref={slidersRef} size={14} className="shrink-0" />
                  ) : (
                    Icon && <Icon className={cn("size-3.5 shrink-0", ICON_HOVER_ANIMATION[section.id])} />
                  )}
                  {section.label}
                </button>
              </li>
            )
          })}
        </ul>
      </nav>

      <div className="min-w-0 flex-1 overflow-hidden rounded-lg border border-border/60 border-t-primary/70 border-t-2 bg-card/70">
      <div className="relative h-full min-h-0">
      <div
        key={active}
        ref={scrollRef}
        onScroll={checkFade}
        className="animate-view-in flex h-full min-h-0 flex-col gap-4 overflow-y-auto p-4 scrollbar-slim"
      >
      {active === "general" && (
        <SettingsSection title="About" description="Version and changelogs.">
          <SettingsGroup>
            <ActionRow
            label="Version"
            hint={
              appVersion
                ? updateStatus === "current"
                ? `v${appVersion} - up to date`
                  : updateStatus === "available"
                    ? `v${appVersion} - v${latestVersion} available`
                    : updateStatus === "error"
                      ? `v${appVersion} - couldn't check`
                      : `v${appVersion}`
                : "Loading…"
            }
            buttonLabel={updateStatus === "checking" ? "Checking…" : "Check for Update"}
            onAction={handleCheckForUpdate}
            disabled={updateStatus === "checking"}
          />
            <Separator />
            <ChangelogList entries={CHANGELOG} />
          </SettingsGroup>
        </SettingsSection>
      )}

      {active === "windows" && (
            <SettingsSection
              title="Windows"
              description="Control how the app behaves alongside Windows."
            >
              <SettingsGroup
                title="Startup"
                description="What happens when Windows starts."
              >
                <ToggleRow
                  label="Launch at startup"
                  hint="Start automatically when you sign in."
                  checked={windowsSettings.launchAtStartup}
                  onCheckedChange={(checked) => onWindowsSettingsChange({ launchAtStartup: checked })}
                />
                <Separator />
                <SegmentedRow
                  label="On startup"
                  hint="How the window should appear the moment it launches."
                  value={windowsSettings.startupState}
                  onValueChange={(value) =>
                    onWindowsSettingsChange({ startupState: value as WindowsSettings["startupState"] })
                  }
                  options={STARTUP_STATE_OPTIONS}
                  disabled={!windowsSettings.launchAtStartup}
                />
                <Separator />
                <SliderRow
                  label="Startup delay"
                  hint="Wait this long after sign-in before launching, so audio drivers have time to settle."
                  value={windowsSettings.startupDelay}
                  onValueChange={(value) => onWindowsSettingsChange({ startupDelay: value })}
                  min={1}
                  max={15}
                  unit="s"
                  disabled={!windowsSettings.launchAtStartup}
                />
              </SettingsGroup>

              <SettingsGroup
                title="App behavior"
                description="General window behavior, outside of startup."
              >
                <ToggleRow
                  label="Minimize to tray"
                  hint="Pressing the close button hides the window instead of quitting."
                  checked={windowsSettings.minimizeToTrayOnClose}
                  onCheckedChange={(checked) => onWindowsSettingsChange({ minimizeToTrayOnClose: checked })}
                />
                <Separator />
                <ToggleRow
                  label="Remember window position"
                  hint="Reopen in the same spot on screen it was last in."
                  checked={windowsSettings.rememberWindowPosition}
                  onCheckedChange={(checked) => onWindowsSettingsChange({ rememberWindowPosition: checked })}
                />
                <Separator />
                <ToggleRow
                  label="Always on top"
                  hint="Keep the window above other apps."
                  checked={windowsSettings.alwaysOnTop}
                  onCheckedChange={(checked) => onWindowsSettingsChange({ alwaysOnTop: checked })}
                />
              </SettingsGroup>
            </SettingsSection>
          )}

{active === "behavior" && (
  <SettingsSection
    title="Behavior"
    description="Feedback and behavior around muting."
  >
              <SettingsGroup
                title="Mute popup"
                description="An on-screen indicator when mute state changes."
              >
                <ToggleRow
                  label="Show mute/unmute popup"
                  hint="Briefly display an indicator whenever the mic is muted or unmuted."
                  checked={systemSettings.popupEnabled}
                  onCheckedChange={(checked) => onSystemSettingsChange({ popupEnabled: checked })}
                />
                <Separator />
                <SelectRow
                  label="Popup location"
                  hint="Where the indicator appears on screen."
                  value={systemSettings.popupLocation}
                  onValueChange={(value) => onSystemSettingsChange({ popupLocation: value })}
                  options={POPUP_LOCATION_OPTIONS}
                  disabled={!systemSettings.popupEnabled}
                />
              </SettingsGroup>

              <SettingsGroup
                title="Mute sound"
                description="An audible cue when mute state changes."
              >
                <ToggleRow
                  label="Play mute/unmute sound"
                  hint="Play a short tone whenever the mic is muted or unmuted."
                  checked={systemSettings.soundEnabled}
                  onCheckedChange={(checked) => onSystemSettingsChange({ soundEnabled: checked })}
                />
                <Separator />
                <SliderRow
                  label="Sound volume"
                  hint="How loud the mute/unmute tone plays."
                  value={systemSettings.soundVolume}
                  onValueChange={(value) => onSystemSettingsChange({ soundVolume: value })}
                  min={0}
                  max={100}
                  step={5}
                  unit="%"
                  disabled={!systemSettings.soundEnabled}
                />
              </SettingsGroup>

              <SettingsGroup
                title="Auto-mute"
                description="Mute automatically after a period of inactivity."
              >
                <ToggleRow
                  label="Auto-mute when idle"
                  hint="Mute the mic once there's been no keyboard or mouse activity for a while."
                  checked={systemSettings.autoMuteEnabled}
                  onCheckedChange={(checked) => onSystemSettingsChange({ autoMuteEnabled: checked })}
                />
                <Separator />
                <SliderRow
                  label="Idle time"
                  hint="How long to wait before auto-muting."
                  value={systemSettings.autoMuteMinutes}
                  onValueChange={(value) => onSystemSettingsChange({ autoMuteMinutes: value })}
                  min={1}
                  max={30}
                  unit=" min"
                  disabled={!systemSettings.autoMuteEnabled}
                />
              </SettingsGroup>
            </SettingsSection>
          )}

          {active === "appearance" && (
            <SettingsSection
              title="Appearance"
              description="Personalize the look and feel."
            >
              <SettingsGroup>
                <SegmentedRow
                  label="Theme"
                  hint="Follow a fixed light or dark appearance."
                  value={appearanceSettings.theme}
                  onValueChange={(value) =>
                    onAppearanceSettingsChange({ theme: value as AppearanceSettings["theme"] })
                  }
                  options={THEME_OPTIONS}
                />
                <Separator />
                <AccentColorRow
                  color={appearanceSettings.accentColor}
                  themeDefaultHex={THEME_PRIMARY_HEX[appearanceSettings.theme]}
                  onColorChange={(value) => onAppearanceSettingsChange({ accentColor: value })}
                />
              </SettingsGroup>

              <SettingsGroup>
                <ToggleRow
                  label="Reduce motion"
                  hint="Disable transitions and micro-animations."
                  checked={appearanceSettings.reduceMotion}
                  onCheckedChange={(checked) => onAppearanceSettingsChange({ reduceMotion: checked })}
                />
              </SettingsGroup>
            </SettingsSection>
          )}

{active === "keybinds" && (
            <SettingsSection
              title="Keybinds"
              description="Global shortcuts that work even while another app is focused."
            >
              <SettingsGroup>
                <ToggleRow
                  label="Enable global hotkeys"
                  hint="Turn all hotkeys below on or off at once."
                  checked={keybindsSettings.hotkeysEnabled}
                  onCheckedChange={(checked) => onKeybindsSettingsChange({ hotkeysEnabled: checked })}
                />
                <Separator />
                <SelectRow
                  label="Mute hotkey trigger"
                  hint="Toggle flips mute on each press. Push to talk unmutes while held."
                  value={keybindsSettings.hotkeyMode}
                  onValueChange={(value) => onKeybindsSettingsChange({ hotkeyMode: value as "toggle" | "push-to-talk" })}
                  options={[
                    { value: "toggle", label: "Toggle" },
                    { value: "push-to-talk", label: "Push to talk" },
                  ]}
                  disabled={!keybindsSettings.hotkeysEnabled}
                />
                <Separator />
                <HotkeyRow
                  label="Mute / unmute"
                  hint="Toggles the microphone, same as the button on the main page."
                  chord={keybindsSettings.muteChord}
                  onChordChange={(chord) => onKeybindsSettingsChange({ muteChord: chord })}
                  disabled={!keybindsSettings.hotkeysEnabled}
                />
                <Separator />
                <HotkeyRow
                  label="Open app"
                  hint="Brings the window to the front from anywhere."
                  chord={keybindsSettings.openAppChord}
                  onChordChange={(chord) => onKeybindsSettingsChange({ openAppChord: chord })}
                  disabled={!keybindsSettings.hotkeysEnabled}
                />
              </SettingsGroup>
            </SettingsSection>
          )}

          {active === "danger" && (
            <SettingsSection
            title="Danger Zone"
            description="Reset your settings"
          >
              <SettingsGroup>
                <ResetAllRow onConfirm={handleResetAll} />
              </SettingsGroup>

              {funModeUnlocked && (
                <div className="animate-view-in flex flex-col gap-3">
                  <SettingsGroup>
                    <ToggleRow
                      label="useless button"
                      labelSuffix={<Sparkles className="ml-1.5 inline size-3 text-primary" />}
                      hint="fun."
                      checked={funModeEnabled}
                      onCheckedChange={onFunModeEnabledChange}
                    />
                  </SettingsGroup>

                  {funModeEnabled && (
                    <SettingsGroup
                    title="setting."
                    description="useless settings"
                  >
                      <ToggleRow
                        label="colors."
                        hint="The accent color continuously cycles through the rainbow."
                        checked={crazyColors}
                        onCheckedChange={onCrazyColorsChange}
                      />
                      <Separator />
                      <ToggleRow
                        label="randomly mute"
                        labelSuffix={
                          <span className="ml-1.5 text-[10px] font-normal text-muted-foreground">
                            (not recommended)
                          </span>
                        }
                        hint="Mutes the mic at a random interval, every 30–90 seconds."
                        checked={randomMute}
                        onCheckedChange={onRandomMuteChange}
                      />
                      <Separator />
                      <ToggleRow
                        label="normal volume"
                        hint="Your volume slider periodically changes on its own, every 3–10 seconds."
                        checked={normalVolume}
                        onCheckedChange={onNormalVolumeChange}
                      />
                    </SettingsGroup>
                  )}
                </div>
              )}
            </SettingsSection>
          )}
        </div>
        <div aria-hidden className={cn("scroll-fade-bottom", showFade && "is-visible")} />
        </div>
      </div>
    </div>
  )
}
