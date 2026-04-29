// demonstrates: per-API system snippets shown in docs/src/system/*.md
// docs: docs/src/system/overview.md
// platforms: macos, linux, windows
// run: false

// `run: false` because most system APIs are interactive (open URLs, ask for
// keychain permission, present notification banners) — they can't run to a
// clean exit under the doc-tests harness's 10-second timeout. We still
// compile-link the file on every PR, which is enough to catch API drift.
//
// We import an `App` from perry/ui so the linker pulls in libperry_ui_*,
// which is where the perry/system FFI symbols live (audio, notifications,
// keychain, preferences, dark-mode and device introspection are all part of
// the platform UI layer).

import { App, VStack, Text } from "perry/ui"
import {
    isDarkMode,
    getDeviceModel, getDeviceIdiom,
    getLocale, getAppIcon,
    openURL,
    keychainSave, keychainGet, keychainDelete,
    preferencesGet, preferencesSet,
    notificationSend, notificationCancel,
    notificationRegisterRemote, notificationOnReceive,
    notificationOnBackgroundReceive, notificationOnTap,
    audioStart, audioStop, audioGetLevel, audioGetPeak, audioGetWaveform,
} from "perry/system"

// ANCHOR: imports
// import {
//     openURL, isDarkMode,
//     preferencesGet, preferencesSet,
//     keychainSave, keychainGet, keychainDelete,
//     notificationSend,
//     audioStart, audioStop, audioGetLevel, audioGetPeak, audioGetWaveform,
// } from "perry/system"
// ANCHOR_END: imports

// ANCHOR: dark-mode
if (isDarkMode()) {
    console.log("Dark mode is active")
}
// ANCHOR_END: dark-mode

// ANCHOR: device
console.log(`device idiom: ${getDeviceIdiom()}`)
console.log(`device model: ${getDeviceModel()}`)
// ANCHOR_END: device

// ANCHOR: locale
const locale = getLocale()
console.log(`locale: ${locale}`)
// ANCHOR_END: locale

// ANCHOR: app-icon
// Returns a Widget handle for the app icon image.
// Pass "" to use the default bundle icon; pass a specific asset path on disk otherwise.
const iconWidget = getAppIcon("")
console.log(`icon widget handle: ${iconWidget}`)
// ANCHOR_END: app-icon

// ANCHOR: open-url
openURL("https://example.com")
// ANCHOR_END: open-url

// ANCHOR: preferences
// Strings and numbers round-trip natively — no manual stringification needed.
preferencesSet("theme", "dark")
preferencesSet("font-size", 14)

const theme = preferencesGet("theme")        // string | number | undefined
const fontSize = preferencesGet("font-size") // → 14 (number)

if (typeof theme === "string") {
    console.log(`saved theme: ${theme}`)
}
if (typeof fontSize === "number") {
    console.log(`saved font-size: ${fontSize}`)
}
// ANCHOR_END: preferences

// ANCHOR: keychain
keychainSave("api_token", "sk-...")
const token = keychainGet("api_token")
keychainDelete("api_token")
console.log(`token length: ${token.length}`)
// ANCHOR_END: keychain

// ANCHOR: notification-send
notificationSend("Build complete", "All targets compiled in 4.2s.")
// ANCHOR_END: notification-send

// ANCHOR: notification-tap
notificationOnTap((id: string, action?: string) => {
    console.log(`tapped notification ${id}; action=${action ?? "(default)"}`)
})
// ANCHOR_END: notification-tap

// ANCHOR: notification-cancel
notificationCancel("daily-reminder")
// ANCHOR_END: notification-cancel

// ANCHOR: notification-remote
notificationRegisterRemote((token: string) => {
    console.log(`APNs device token: ${token}`)
})

notificationOnReceive((payload: object) => {
    console.log(`got remote payload: ${JSON.stringify(payload)}`)
})
// ANCHOR_END: notification-remote

// ANCHOR: notification-background
// Background delivery (#98). The OS runs this when a remote notification
// arrives and the app is backgrounded (or terminated, on iOS). The
// returned Promise gates iOS's `UIBackgroundFetchResult` signal — keeping
// the process alive until the work is actually done.
notificationOnBackgroundReceive(async (payload: object) => {
    // Mirror the payload locally so the next foreground launch can show it.
    preferencesSet("last-bg-payload", JSON.stringify(payload))
    // Real apps would hit a server here:
    //   await fetch(`https://api.example.com/ack`, { method: "POST", body: ... })
})
// ANCHOR_END: notification-background

// ANCHOR: audio
const ok = audioStart() // 1 on success, 0 on failure
if (ok === 1) {
    const level = audioGetLevel()           // 0..1
    const peak = audioGetPeak()             // 0..1
    const waveform = audioGetWaveform(64)   // sample-count
    console.log(`level=${level} peak=${peak} waveform=${waveform}`)
    audioStop()
}
// ANCHOR_END: audio

App({
    title: "system-snippets",
    width: 320,
    height: 200,
    body: VStack(8, [Text("compile-only example for perry/system docs")]),
})
