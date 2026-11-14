type CueKind = "mute" | "unmute"

const SOUND_PATHS: Record<CueKind, string> = {
  mute: "/sounds/mute.ogg",
  unmute: "/sounds/unmute.ogg",
}

let sharedContext: AudioContext | null = null
const decodedCache: Partial<Record<CueKind, AudioBuffer>> = {}
const pendingDecodes: Partial<Record<CueKind, Promise<AudioBuffer>>> = {}
let activeSource: AudioBufferSourceNode | null = null
let activeFallbackAudio: HTMLAudioElement | null = null
let playSequence = 0

function getAudioContext(): AudioContext | null {
  if (typeof window === "undefined") return null
  if (!sharedContext) {
    const AudioCtx =
      window.AudioContext ||
      (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext
    sharedContext = new AudioCtx()
  }
  return sharedContext
}

function loadCueBuffer(kind: CueKind): Promise<AudioBuffer> {
  const cached = decodedCache[kind]
  if (cached) return Promise.resolve(cached)

  const inFlight = pendingDecodes[kind]
  if (inFlight) return inFlight

  const ctx = getAudioContext()
  if (!ctx) return Promise.reject(new Error("AudioContext unavailable"))

  const promise = fetch(SOUND_PATHS[kind])
    .then((response) => {
      if (!response.ok) throw new Error(`failed to load ${SOUND_PATHS[kind]} (${response.status})`)
      return response.arrayBuffer()
    })
    .then((bytes) => ctx.decodeAudioData(bytes))
    .then((decoded) => {
      decodedCache[kind] = decoded
      return decoded
    })
    .finally(() => {
      delete pendingDecodes[kind]
    })

  pendingDecodes[kind] = promise
  return promise
}

function stopCurrentPlayback() {
  if (activeSource) {
    try {
      activeSource.stop()
    } catch {
      // already finished
    }
    activeSource.disconnect()
    activeSource = null
  }
  if (activeFallbackAudio) {
    activeFallbackAudio.pause()
    activeFallbackAudio.currentTime = 0
    activeFallbackAudio = null
  }
}

export function preloadMuteSounds() {
  if (typeof window === "undefined") return
  void loadCueBuffer("mute").catch(() => {})
  void loadCueBuffer("unmute").catch(() => {})
}

export async function playMuteSound(nowMuted: boolean, volume: number) {
  const gainLevel = Math.max(0, Math.min(1, volume / 100))
  if (gainLevel <= 0.0001) return

  const kind: CueKind = nowMuted ? "mute" : "unmute"
  const mySequence = ++playSequence

  try {
    const buffer = await loadCueBuffer(kind)
    const ctx = getAudioContext()
    if (!ctx || mySequence !== playSequence) return

    if (ctx.state === "suspended") {
      await ctx.resume()
      if (mySequence !== playSequence) return
    }

    stopCurrentPlayback()
    const source = ctx.createBufferSource()
    const gain = ctx.createGain()
    source.buffer = buffer
    gain.gain.setValueAtTime(gainLevel, ctx.currentTime)
    source.connect(gain)
    gain.connect(ctx.destination)
    source.onended = () => {
      if (activeSource === source) activeSource = null
      source.disconnect()
      gain.disconnect()
    }
    activeSource = source
    source.start()
  } catch {
    if (mySequence !== playSequence) return
    stopCurrentPlayback()
    const fallback = new Audio(SOUND_PATHS[kind])
    fallback.volume = gainLevel
    activeFallbackAudio = fallback
    fallback.play().catch(() => {})
  }
}
