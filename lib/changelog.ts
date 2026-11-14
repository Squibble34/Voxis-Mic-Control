export type ChangelogEntry = {
    version: string
    date: string
    sections: { label: string; items: string[] }[]
  }
  
  export const CHANGELOG: ChangelogEntry[] = [
    {
      version: "2.0.0",
      date: "Nov 14, 2026",
      sections: [
        {
          label: "Added",
          items: [
            "`Voxis Microphone` virtual USB audio device powered by `usbip-win2`, automatically installs usbip when installing voxis.",
            "Noise suppression dropdown in the microphone section (`Off`, `RNNoise`, and `DeepFilterNet`) with CPU overload fallback protection (if cpu gets overloaded and cant support DFN noise suppression)",
            "Live `Listen` sidetone toggle in the input level section to listen your processed microphone audio.",
            "Adjustable `Noise Gate` with live threshold marker on the input level meter",
            "`Voice Compressor` auto leveler to lift quiet speech and lower loud audio peaks",
            "`Voice Tone` EQ presets (`Natural`, `Warm`, `Crisp`) and an 80 Hz low-cut rumble filter",
            "`Boost volume up to 200%` setting with a soft-knee limiter to prevent harsh digital clipping",
            "`Talking while muted` detection that warns you in the by flashing a popup when you start talking while muted",
            "Startup loading screen so the window opens cleanly without visible layout shifts",
          ],
        },
        {
          label: "Changed",
          items: [
            "Elevated audio and USB/IP threads to `Pro Audio` real-time scheduling and disabled Windows 11 Efficiency Mode throttling so heavy games cannot starve the virtual mic",
            "Moved the noise suppression selector into a compact dropdown in the top-right corner of the microphone card",
            "Changed the mute/unmute sound to be more distinct from each other"
          ],
        },
        {
          label: "Fixed",
          items: [
            "Reopening to the previous window position now persists and restores coordinates accurately",
            "Enforced a single app instance so launching Voxis while it is already running brings the existing window to the front instead of opening a second window",
            "Fixed the mute/unmute popup occasionally losing its always-on-top z-order over fullscreen or borderless windows",
            "Microphone access tracker now removes apps after they stop actively capturing audio",
            "Check for update button not letting you install the update after it finds an update",
          ],
        },
      ],
    },
    {
        version: "1.1.0",
        date: "Aug 19, 2026",
        sections: [
          {
            label: "Added",
            items: [
              "New `General` settings section with app version and a changelog viewer",
              "`Check for Update` button",
            ],
          },
          {
            label: "Changed",
            items: [
              "Reworked the mute/unmute popup UI - solid background, colored border, and a one-shot pulse on unmute instead of a static tint",
              "Shortened the mute/unmute hotkey cooldown from 300ms to 50ms",
            ],
          },
          {
            label: "Fixed",
            items: [
              "`Open app` hotkey no longer fails to restore the window when it's minimized",
            ],
          },
        ],
      },
    {
      version: "1.0.0",
      date: "Jul 22, 2026",
      sections: [
        { label: "Finished", items: ["first version done"] },
      ],
    },
  ]