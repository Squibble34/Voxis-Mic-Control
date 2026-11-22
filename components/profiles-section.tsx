"use client"

import * as React from "react"
import {
  ArrowDown,
  ArrowUp,
  Check,
  Download,
  Pencil,
  Plus,
  Star,
  Trash2,
  Upload,
  X,
} from "lucide-react"
import type { TrackedApp } from "@/components/app-tracker-card"
import {
  DEFAULT_APPEARANCE_SETTINGS,
  DEFAULT_KEYBINDS_SETTINGS,
  DEFAULT_NOISE_SETTINGS,
  DEFAULT_SYSTEM_SETTINGS,
  DEFAULT_WINDOWS_SETTINGS,
  type ProfileRule,
  type ProfileSnapshot,
  type ProfilesConfig,
  type VoxisProfile,
} from "@/lib/settings-types"
import { Button } from "@/components/ui/button"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Separator } from "@/components/ui/separator"
import { Switch } from "@/components/ui/switch"
import { SettingsGroup, SettingsSection, ToggleRow } from "@/components/settings-controls"
import { cn } from "@/lib/utils"

function makeId(prefix: string): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return `${prefix}-${crypto.randomUUID().slice(0, 8)}`
  }
  return `${prefix}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`
}

function normalizeExeName(raw: string): string {
  return raw.trim().toLowerCase().replace(/\.exe$/i, "")
}

function normalizeImportedSnapshot(raw: unknown): ProfileSnapshot | null {
  if (!raw || typeof raw !== "object") return null
  const obj = raw as Record<string, unknown>
  if (
    !obj.systemSettings &&
    !obj.appearanceSettings &&
    !obj.keybindsSettings &&
    !obj.windowsSettings &&
    !obj.noiseSettings
  ) {
    return null
  }
  return {
    systemSettings: {
      ...DEFAULT_SYSTEM_SETTINGS,
      ...((obj.systemSettings as object) ?? {}),
    },
    appearanceSettings: {
      ...DEFAULT_APPEARANCE_SETTINGS,
      ...((obj.appearanceSettings as object) ?? {}),
    },
    keybindsSettings: {
      ...DEFAULT_KEYBINDS_SETTINGS,
      ...((obj.keybindsSettings as object) ?? {}),
    },
    windowsSettings: {
      ...DEFAULT_WINDOWS_SETTINGS,
      ...((obj.windowsSettings as object) ?? {}),
    },
    noiseSettings: {
      ...DEFAULT_NOISE_SETTINGS,
      ...((obj.noiseSettings as object) ?? {}),
    },
    softwareGain: typeof obj.softwareGain === "number" ? obj.softwareGain : 1,
  }
}

export function ProfilesSection({
  profilesConfig,
  currentSnapshot,
  trackedApps,
  onProfilesConfigChange,
  onApplyProfile,
}: {
  profilesConfig: ProfilesConfig
  currentSnapshot: ProfileSnapshot
  trackedApps: TrackedApp[]
  onProfilesConfigChange: (next: ProfilesConfig) => void
  onApplyProfile: (profileId: string) => void
}) {
  const [newProfileName, setNewProfileName] = React.useState("")
  const [importStatus, setImportStatus] = React.useState<{
    kind: "ok" | "err"
    text: string
  } | null>(null)
  const fileInputRef = React.useRef<HTMLInputElement | null>(null)

  const [renamingId, setRenamingId] = React.useState<string | null>(null)
  const [renameDraft, setRenameDraft] = React.useState("")
  const [confirmDeleteId, setConfirmDeleteId] = React.useState<string | null>(null)

  const [ruleAppInput, setRuleAppInput] = React.useState("")
  const [ruleProfileId, setRuleProfileId] = React.useState(
    () => profilesConfig.profiles[0]?.id ?? "",
  )

  React.useEffect(() => {
    if (!confirmDeleteId) return
    const t = window.setTimeout(() => setConfirmDeleteId(null), 3500)
    return () => window.clearTimeout(t)
  }, [confirmDeleteId])

  React.useEffect(() => {
    if (!ruleProfileId || !profilesConfig.profiles.some((p) => p.id === ruleProfileId)) {
      setRuleProfileId(profilesConfig.profiles[0]?.id ?? "")
    }
  }, [profilesConfig.profiles, ruleProfileId])

  function handleSaveCurrentProfile(e: React.FormEvent) {
    e.preventDefault()
    const trimmed = newProfileName.trim()
    if (!trimmed) return
    const created: VoxisProfile = {
      id: makeId("prof"),
      name: trimmed,
      snapshot: structuredClone(currentSnapshot),
    }
    const nextProfiles = [...profilesConfig.profiles, created]
    onProfilesConfigChange({
      ...profilesConfig,
      profiles: nextProfiles,
      activeProfileId: created.id,
      defaultProfileId: profilesConfig.defaultProfileId ?? created.id,
    })
    setNewProfileName("")
    setImportStatus({ kind: "ok", text: `Saved "${created.name}".` })
  }

  function handleImportFile(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0]
    e.target.value = ""
    if (!file) return

    const reader = new FileReader()
    reader.onload = () => {
      try {
        const parsed = JSON.parse(String(reader.result ?? "")) as Record<string, unknown>
        const candidate = (parsed.profile as Record<string, unknown> | undefined) ?? parsed
        const snapshot = normalizeImportedSnapshot(candidate.snapshot ?? candidate)
        if (!snapshot) {
          setImportStatus({ kind: "err", text: "Invalid profile JSON file." })
          return
        }
        const rawName =
          typeof candidate.name === "string" && candidate.name.trim()
            ? candidate.name.trim()
            : file.name.replace(/\.voxis-profile\.json$|\.json$/i, "") || "Imported profile"
        const imported: VoxisProfile = {
          id: makeId("prof"),
          name: rawName,
          snapshot,
        }
        onProfilesConfigChange({
          ...profilesConfig,
          profiles: [...profilesConfig.profiles, imported],
          defaultProfileId: profilesConfig.defaultProfileId ?? imported.id,
        })
        setImportStatus({ kind: "ok", text: `Imported "${imported.name}".` })
      } catch {
        setImportStatus({ kind: "err", text: "Could not read profile JSON file." })
      }
    }
    reader.readAsText(file)
  }

  function handleExportProfile(profile: VoxisProfile) {
    const payload = JSON.stringify(
      {
        version: 1,
        name: profile.name,
        snapshot: profile.snapshot,
      },
      null,
      2,
    )
    const blob = new Blob([payload], { type: "application/json" })
    const url = URL.createObjectURL(blob)
    const slug =
      profile.name
        .toLowerCase()
        .replace(/[^a-z0-9-_]+/g, "-")
        .replace(/^-|-$/g, "") || "profile"
    const link = document.createElement("a")
    link.href = url
    link.download = `${slug}.voxis-profile.json`
    document.body.appendChild(link)
    link.click()
    link.remove()
    URL.revokeObjectURL(url)
  }

  function startRename(profile: VoxisProfile) {
    setRenamingId(profile.id)
    setRenameDraft(profile.name)
    setConfirmDeleteId(null)
  }

  function commitRename(profileId: string) {
    const trimmed = renameDraft.trim()
    if (!trimmed) return
    onProfilesConfigChange({
      ...profilesConfig,
      profiles: profilesConfig.profiles.map((p) =>
        p.id === profileId ? { ...p, name: trimmed } : p,
      ),
    })
    setRenamingId(null)
  }

  function handleDeleteClick(profileId: string) {
    if (confirmDeleteId !== profileId) {
      setConfirmDeleteId(profileId)
      return
    }
    const remainingProfiles = profilesConfig.profiles.filter((p) => p.id !== profileId)
    const remainingRules = profilesConfig.rules.filter((r) => r.profileId !== profileId)
    onProfilesConfigChange({
      ...profilesConfig,
      profiles: remainingProfiles,
      rules: remainingRules,
      defaultProfileId:
        profilesConfig.defaultProfileId === profileId ? null : profilesConfig.defaultProfileId,
      activeProfileId:
        profilesConfig.activeProfileId === profileId ? null : profilesConfig.activeProfileId,
    })
    setConfirmDeleteId(null)
  }

  function handleSetDefault(profileId: string) {
    onProfilesConfigChange({
      ...profilesConfig,
      defaultProfileId: profilesConfig.defaultProfileId === profileId ? null : profileId,
    })
  }

  function handleAddRule(e: React.FormEvent) {
    e.preventDefault()
    const normalized = normalizeExeName(ruleAppInput)
    if (!normalized || !ruleProfileId) return
    const matchingTracked = trackedApps.find(
      (app) => normalizeExeName(app.name) === normalized,
    )
    const rule: ProfileRule = {
      id: makeId("rule"),
      appMatch: normalized,
      appLabel: matchingTracked?.name ?? ruleAppInput.trim(),
      profileId: ruleProfileId,
      enabled: true,
    }
    onProfilesConfigChange({
      ...profilesConfig,
      rules: [...profilesConfig.rules, rule],
    })
    setRuleAppInput("")
  }

  function moveRule(index: number, delta: -1 | 1) {
    const nextIndex = index + delta
    if (nextIndex < 0 || nextIndex >= profilesConfig.rules.length) return
    const copy = [...profilesConfig.rules]
    const [moved] = copy.splice(index, 1)
    copy.splice(nextIndex, 0, moved)
    onProfilesConfigChange({ ...profilesConfig, rules: copy })
  }

  function updateRule(ruleId: string, patch: Partial<ProfileRule>) {
    onProfilesConfigChange({
      ...profilesConfig,
      rules: profilesConfig.rules.map((r) => (r.id === ruleId ? { ...r, ...patch } : r)),
    })
  }

  function removeRule(ruleId: string) {
    onProfilesConfigChange({
      ...profilesConfig,
      rules: profilesConfig.rules.filter((r) => r.id !== ruleId),
    })
  }

  const knownApps = React.useMemo(() => {
    const seen = new Set<string>()
    const list: string[] = []
    for (const app of trackedApps) {
      const norm = normalizeExeName(app.name)
      if (!seen.has(norm)) {
        seen.add(norm)
        list.push(app.name)
      }
    }
    return list
  }, [trackedApps])

  const profileOptions = React.useMemo(
    () => profilesConfig.profiles.map((p) => ({ value: p.id, label: p.name })),
    [profilesConfig.profiles],
  )

  const appOptions = React.useMemo(
    () => knownApps.map((appName) => ({ value: appName, label: appName })),
    [knownApps],
  )

  return (
    <SettingsSection
      title="Profiles"
      description="Save complete setting presets and switch automatically by capturing app."
    >
      <SettingsGroup
        title="Save & import"
        description="Turn your current settings into a reusable preset or import one from JSON."
      >
        <form onSubmit={handleSaveCurrentProfile} className="flex items-center gap-2">
          <input
            type="text"
            value={newProfileName}
            onChange={(e) => setNewProfileName(e.target.value)}
            placeholder="Profile name (e.g. Discord, Streaming)"
            maxLength={48}
            className="h-8 min-w-0 flex-1 rounded-md border border-border/70 bg-background/70 px-2.5 text-xs text-foreground placeholder:text-muted-foreground/70 outline-none focus-visible:border-primary/60 focus-visible:ring-2 focus-visible:ring-primary/25"
          />
          <Button
            type="submit"
            size="sm"
            disabled={!newProfileName.trim()}
            className="h-8 shrink-0 gap-1.5 px-3 text-xs"
          >
            <Plus className="size-3.5" />
            Save
          </Button>
        </form>
        <Separator />
        <div className="flex items-center justify-between gap-3">
          <div className="flex min-w-0 flex-col gap-0.5">
            <span className="text-[13px] font-medium text-foreground">Import preset</span>
            <span className="text-xs text-muted-foreground">
              Load an exported `.voxis-profile.json` file.
            </span>
          </div>
          <input
            ref={fileInputRef}
            type="file"
            accept=".json,application/json"
            onChange={handleImportFile}
            className="hidden"
          />
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => fileInputRef.current?.click()}
            className="h-8 shrink-0 gap-1.5 px-3 text-xs"
          >
            <Upload className="size-3.5" />
            Import
          </Button>
        </div>
        {importStatus && (
          <p
            className={cn(
              "text-[11px]",
              importStatus.kind === "err" ? "text-destructive" : "text-primary",
            )}
          >
            {importStatus.text}
          </p>
        )}
      </SettingsGroup>

      <SettingsGroup
        title="Saved presets"
        description="Apply any preset, pick your Default fallback profile, rename, export, or delete."
      >
        {profilesConfig.profiles.length === 0 ? (
          <p className="text-xs text-muted-foreground">
            No profiles saved yet. Save your current settings above to create your first preset.
          </p>
        ) : (
          <div className="flex flex-col gap-2">
            {profilesConfig.profiles.map((profile) => {
              const isActive = profilesConfig.activeProfileId === profile.id
              const isDefault = profilesConfig.defaultProfileId === profile.id
              const isRenaming = renamingId === profile.id
              const isConfirmDelete = confirmDeleteId === profile.id

              return (
                <div
                  key={profile.id}
                  className={cn(
                    "flex flex-col gap-2 rounded-md border px-3 py-2.5 transition-colors",
                    isActive
                      ? "border-primary/50 bg-primary/8"
                      : "border-border/60 bg-background/40",
                  )}
                >
                  {isRenaming ? (
                    <div className="flex items-center gap-1.5">
                      <input
                        type="text"
                        value={renameDraft}
                        onChange={(e) => setRenameDraft(e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") commitRename(profile.id)
                          if (e.key === "Escape") setRenamingId(null)
                        }}
                        autoFocus
                        maxLength={48}
                        className="h-7 min-w-0 flex-1 rounded border border-border/80 bg-background px-2 text-xs text-foreground outline-none focus-visible:border-primary/60"
                      />
                      <Button
                        type="button"
                        size="sm"
                        onClick={() => commitRename(profile.id)}
                        disabled={!renameDraft.trim()}
                        className="h-7 px-2 text-[11px]"
                      >
                        <Check className="size-3.5" />
                      </Button>
                      <Button
                        type="button"
                        variant="ghost"
                        size="sm"
                        onClick={() => setRenamingId(null)}
                        className="h-7 px-2 text-[11px]"
                      >
                        <X className="size-3.5" />
                      </Button>
                    </div>
                  ) : (
                    <>
                      <div className="flex min-w-0 items-center justify-between gap-2">
                        <span className="truncate text-[13px] font-medium text-foreground">
                          {profile.name}
                        </span>
                        <div className="flex shrink-0 items-center gap-1">
                          {isActive && (
                            <span className="rounded bg-primary/15 px-1.5 py-0.5 text-[10px] font-medium text-primary">
                              Active
                            </span>
                          )}
                          {isDefault && (
                            <span className="rounded bg-muted px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground">
                              Default
                            </span>
                          )}
                        </div>
                      </div>

                      <div className="flex flex-wrap items-center gap-1.5">
                        <Button
                          type="button"
                          variant={isActive ? "secondary" : "outline"}
                          size="sm"
                          onClick={() => onApplyProfile(profile.id)}
                          className="h-7 px-2.5 text-[11px]"
                        >
                          Apply
                        </Button>
                        <Button
                          type="button"
                          variant="ghost"
                          size="sm"
                          onClick={() => handleSetDefault(profile.id)}
                          title={isDefault ? "Clear default fallback" : "Set as default fallback"}
                          className={cn(
                            "h-7 gap-1 px-2 text-[11px]",
                            isDefault ? "text-primary" : "text-muted-foreground",
                          )}
                        >
                          <Star className={cn("size-3", isDefault && "fill-current")} />
                          Default
                        </Button>
                        <div className="ml-auto flex items-center gap-1">
                          <Button
                            type="button"
                            variant="ghost"
                            size="sm"
                            onClick={() => startRename(profile)}
                            title="Rename preset"
                            className="h-7 px-2 text-[11px] text-muted-foreground hover:text-foreground"
                          >
                            <Pencil className="size-3.5" />
                          </Button>
                          <Button
                            type="button"
                            variant="ghost"
                            size="sm"
                            onClick={() => handleExportProfile(profile)}
                            title="Export JSON"
                            className="h-7 px-2 text-[11px] text-muted-foreground hover:text-foreground"
                          >
                            <Download className="size-3.5" />
                          </Button>
                          <Button
                            type="button"
                            variant={isConfirmDelete ? "destructive" : "ghost"}
                            size="sm"
                            onClick={() => handleDeleteClick(profile.id)}
                            className={cn(
                              "h-7 gap-1 px-2 text-[11px]",
                              !isConfirmDelete && "text-muted-foreground hover:text-destructive",
                            )}
                          >
                            <Trash2 className="size-3.5" />
                            {isConfirmDelete ? "Confirm?" : "Delete"}
                          </Button>
                        </div>
                      </div>
                    </>
                  )}
                </div>
              )
            })}
          </div>
        )}
      </SettingsGroup>

      <SettingsGroup
        title="Auto-switch conditions"
        description="Highest-priority matching rule wins while that app captures your mic, then reverts to your Default profile."
      >
        <ToggleRow
          label="Auto-switch by capturing app"
          hint="Automatically apply a preset when a matching app is capturing audio."
          checked={profilesConfig.autoSwitchEnabled}
          onCheckedChange={(checked) =>
            onProfilesConfigChange({ ...profilesConfig, autoSwitchEnabled: checked })
          }
        />
        <Separator />

        <form onSubmit={handleAddRule} className="flex flex-col gap-2">
          <div className="flex items-center gap-2">
            <input
              type="text"
              value={ruleAppInput}
              onChange={(e) => setRuleAppInput(e.target.value)}
              placeholder="Process name (e.g. discord, obs64)"
              className="h-8 min-w-0 flex-1 rounded-md border border-border/70 bg-background/70 px-2.5 text-xs text-foreground placeholder:text-muted-foreground/70 outline-none focus-visible:border-primary/60 focus-visible:ring-2 focus-visible:ring-primary/25"
            />
            {knownApps.length > 0 && (
              <Select
                items={appOptions}
                value=""
                onValueChange={(val) => { if (val) setRuleAppInput(val) }}
              >
                <SelectTrigger className="h-8 w-32 shrink-0 text-xs">
                  <SelectValue placeholder="Active apps" />
                </SelectTrigger>
                <SelectContent>
                  <SelectGroup>
                    {knownApps.map((appName) => (
                      <SelectItem key={appName} value={appName}>
                        {appName}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                </SelectContent>
              </Select>
            )}
          </div>

          <div className="flex items-center gap-2">
            <Select
              items={profileOptions}
              value={ruleProfileId}
              onValueChange={(val) => { if (val) setRuleProfileId(val) }}
              disabled={profilesConfig.profiles.length === 0}
            >
              <SelectTrigger className="h-8 min-w-0 flex-1 text-xs">
                <SelectValue placeholder="Choose target profile" />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  {profilesConfig.profiles.map((p) => (
                    <SelectItem key={p.id} value={p.id}>
                      {p.name}
                    </SelectItem>
                  ))}
                </SelectGroup>
              </SelectContent>
            </Select>

            <Button
              type="submit"
              size="sm"
              disabled={!ruleAppInput.trim() || !ruleProfileId}
              className="h-8 shrink-0 gap-1 px-3 text-xs"
            >
              <Plus className="size-3.5" />
              Add
            </Button>
          </div>
        </form>

        {profilesConfig.rules.length > 0 && (
          <div className="flex flex-col gap-2 pt-1">
            {profilesConfig.rules.map((rule, index) => (
              <div
                key={rule.id}
                className="flex flex-col gap-2 rounded-md border border-border/60 bg-background/40 px-3 py-2"
              >
                <div className="flex items-center justify-between gap-2">
                  <div className="flex min-w-0 items-center gap-2">
                    <span className="shrink-0 font-mono text-[11px] text-muted-foreground">
                      #{index + 1}
                    </span>
                    <span className="truncate text-[13px] font-medium text-foreground">
                      {rule.appLabel || rule.appMatch}
                    </span>
                  </div>

                  <div className="flex shrink-0 items-center gap-1">
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      disabled={index === 0}
                      onClick={() => moveRule(index, -1)}
                      title="Move priority up"
                      className="h-7 px-1.5 text-muted-foreground hover:text-foreground disabled:opacity-30"
                    >
                      <ArrowUp className="size-3.5" />
                    </Button>
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      disabled={index === profilesConfig.rules.length - 1}
                      onClick={() => moveRule(index, 1)}
                      title="Move priority down"
                      className="h-7 px-1.5 text-muted-foreground hover:text-foreground disabled:opacity-30"
                    >
                      <ArrowDown className="size-3.5" />
                    </Button>
                    <Switch
                      checked={rule.enabled}
                      onCheckedChange={(checked) => updateRule(rule.id, { enabled: checked })}
                      aria-label={`Enable rule for ${rule.appLabel}`}
                    />
                  </div>
                </div>

                <div className="flex items-center gap-2">
                  <Select
                    items={profileOptions}
                    value={rule.profileId}
                    onValueChange={(val) => { if (val) updateRule(rule.id, { profileId: val }) }}
                  >
                    <SelectTrigger className="h-7 min-w-0 flex-1 text-xs">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectGroup>
                        {profilesConfig.profiles.map((p) => (
                          <SelectItem key={p.id} value={p.id}>
                            {p.name}
                          </SelectItem>
                        ))}
                      </SelectGroup>
                    </SelectContent>
                  </Select>

                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    onClick={() => removeRule(rule.id)}
                    title="Remove condition"
                    className="h-7 shrink-0 px-2 text-muted-foreground hover:text-destructive"
                  >
                    <Trash2 className="size-3.5" />
                  </Button>
                </div>
              </div>
            ))}
          </div>
        )}
      </SettingsGroup>
    </SettingsSection>
  )
}
