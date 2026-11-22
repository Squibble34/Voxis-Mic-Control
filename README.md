# Voxis Mic Control

<img width="646" height="437" alt="image" src="https://github.com/user-attachments/assets/da5ba857-067f-4a13-b3c2-a6ea38a05f02" />


A lightweight Windows microphone control utility and virtual microphone powered by [usbip-win2](https://github.com/vadimgrn/usbip-win2). Voxis creates a virtual USB microphone (`Voxis Microphone`) that routes your physical mic through an onboard DSP and AI noise suppression pipeline before other apps receive it, while giving you global mute/unmute hotkeys, volume boost, auto-mute on idle, and a per-app capture tracker from a compact window and system tray icon. App stays at around 150 - 200mb of ram when fully open and stays at 20mb when minimized to tray; alternatives for mic control like steelseries sonar or wave link use 500mb - 1gb of ram.

## Features

- **Virtual USB Microphone (`Voxis Microphone`)** - Emulates a hardware-level USB Audio Class microphone via [usbip-win2](https://github.com/vadimgrn/usbip-win2) with automatic driver setup, self-healing auto-attach, and automatic assignment as the Windows default recording device
- **AI Noise Suppression** - Switch between `Off`, `RNNoise` (low CPU usage neural denoiser), and `DeepFilterNet` (deep learning voice isolation, higher CPU usage)
- **Voice Processing & DSP** - Adjustable noise gate with a live meter marker, voice compressor/auto leveler, `Natural` / `Warm` / `Crisp` voice tone EQ presets, 80 Hz low cut rumble filter, and optional 200% software volume boost with a soft knee limiter
- **Live Sidetone (`Listen`)** - Hear your processed microphone output in real time while tuning your gate, EQ, or suppression mode
- **Global Hotkeys** - Global mute/unmute shortcut (toggle or push-to-talk) and a shortcut to bring the app to the front from anywhere
- **Smart Alerts & Auto-Mute** - On-screen popup indicator when mute state changes, automatic mute after a period of inactivity, and a visual warning when you start talking while muted
- **Live Mic Access Tracker** - See which applications are actively capturing your microphone

## Install

Download the latest installer from the [Releases](https://github.com/Squibble34/Voxis-Mic-Control/releases) page and run it. If the [usbip-win2](https://github.com/vadimgrn/usbip-win2) driver is not already installed on your PC, Voxis will download and install automatically without requiring a system restart.

> **Note:** Since this is a small, unsigned, independently-built app, Windows Defender or SmartScreen may flag it on first run. This is a false positive, not a sign of anything actually malicious, it is just because it is not code signed (which costs a lot). the source is fully open in this repo if you want to verify.

## License

MIT - See [LICENSE](LICENSE).
