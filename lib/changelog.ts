export type ChangelogEntry = {
    version: string
    date: string
    sections: { label: string; items: string[] }[]
  }
  
  export const CHANGELOG: ChangelogEntry[] = [
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