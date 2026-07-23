# Summary

[Introduction](introduction.md)

---

# Getting Started

- [Installation](getting-started/installation.md)
- [Hello World](getting-started/hello-world.md)
- [First Native App](getting-started/first-app.md)
- [Project Configuration](getting-started/project-config.md)

# Language

- [Supported Features](language/supported-features.md)
- [Type System](language/type-system.md)
- [Decorators](language/decorators.md)
- [Limitations](language/limitations.md)

# npm Packages

- [Porting Packages (experimental)](packages/porting.md)
- [Native Bindings — Overview](native-libraries/overview.md)
  - [Authoring a Native Binding](native-libraries/authoring-guide.md)
  - [`perry-ffi` ABI Reference](native-libraries/abi.md)
  - [Manifest Schema (spec v1)](native-libraries/manifest-v1.md)

# Multi-Threading

- [Overview](threading/overview.md)
- [parallelMap](threading/parallel-map.md)
- [parallelFilter](threading/parallel-filter.md)
- [spawn](threading/spawn.md)

# Native UI

- [Overview](ui/overview.md)
- [Widgets](ui/widgets.md)
- [Layout](ui/layout.md)
- [Styling](ui/styling.md)
- [State Management](ui/state.md)
- [Events](ui/events.md)
- [Canvas](ui/canvas.md)
- [Menus](ui/menus.md)
- [Tray Icon](ui/tray.md)
- [Dialogs](ui/dialogs.md)
- [Table](ui/table.md)
- [Animation](ui/animation.md)
- [Frame Callbacks](ui/on-frame.md)
- [Multi-Window](ui/multi-window.md)
- [Theming](ui/theming.md)
- [Camera](ui/camera.md)
- [WebView](ui/webview.md)

# Terminal UI

- [Overview](tui/overview.md)
- [Widgets](tui/widgets.md)
- [Hooks](tui/hooks.md)
- [Examples](tui/examples.md)

# Platforms

- [Overview](platforms/overview.md)
- [macOS](platforms/macos.md)
- [iOS](platforms/ios.md)
- [visionOS](platforms/visionos.md)
- [tvOS](platforms/tvos.md)
- [watchOS](platforms/watchos.md)
  - [Publishing to the App Store](platforms/watchos-app-store.md)
- [Android](platforms/android.md)
- [Wear OS](platforms/wearos.md)
- [HarmonyOS NEXT](platforms/harmonyos.md)
- [Windows](platforms/windows.md)
  - [Windows 7 Compatibility](platforms/windows-7.md)
- [Linux (GTK4)](platforms/linux.md)
- [Web](platforms/web.md)
- [WebAssembly](platforms/wasm.md)

# Standard Library

- [Overview](stdlib/overview.md)
- [File System](stdlib/fs.md)
- [HTTP & Networking](stdlib/http.md)
- [Databases](stdlib/database.md)
- [Cryptography](stdlib/crypto.md)
- [Containers](stdlib/container.md)
- [Utilities](stdlib/utilities.md)
- [Other Modules](stdlib/other.md)
- [API Reference (auto-generated)](api/reference.md)

# Containers

- [Overview](container/overview.md)
- [Single-Container Lifecycle](container/containers.md)
- [Compose Orchestration](container/compose.md)
- [Networking](container/networking.md)
- [Volumes](container/volumes.md)
- [Security](container/security.md)
- [Production Patterns](container/production-patterns.md)

# Internationalization

- [Overview](i18n/overview.md)
- [Interpolation & Plurals](i18n/interpolation.md)
- [Formatting](i18n/formatting.md)
- [CLI Tools](i18n/cli.md)

# Auto-Update

- [Overview](updater/overview.md)

# System APIs

- [Overview](system/overview.md)
- [Preferences](system/preferences.md)
- [Keychain](system/keychain.md)
- [Notifications](system/notifications.md)
- [Audio Capture](system/audio.md)
- [Audio Playback](system/audio_module.md)
- [Media Playback](system/media.md)
- [Geolocation & Image Picker](system/geolocation.md)
- [Background Tasks](system/background.md)
- [Other](system/other.md)

# Widgets

- [Widgets](widgets/overview.md)
  - [Creating Widgets](widgets/creating-widgets.md)
  - [Components & Modifiers](widgets/components.md)
  - [Configuration](widgets/configuration.md)
  - [Data Fetching](widgets/data-fetching.md)
  - [Cross-Platform Reference](widgets/platforms.md)
  - [watchOS Complications](widgets/watchos.md)
  - [Wear OS Tiles](widgets/wearos.md)

# Plugins

- [Overview](plugins/overview.md)
- [Creating Plugins](plugins/creating-plugins.md)
- [Hooks & Events](plugins/hooks-and-events.md)
- [Native Extensions](plugins/native-extensions.md)
- [App Store Review](plugins/appstore-review.md)

# Testing

- [Geisterhand (UI Fuzzer)](testing/geisterhand.md)

# CLI Reference

- [Commands](cli/commands.md)
- [Compiler Flags](cli/flags.md)
- [Fast-math (`--fast-math`)](cli/fast-math.md)
- [Dynamic Stdlib Dispatch](cli/dynamic-dispatch.md)
- [JS Runtime Opt-In](cli/allow-js-runtime.md)
- [`PERRY_SANDBOX_BUILDRS`](cli/sandbox-buildrs.md)
- [`--emit-attest` (binary attestation sidecar)](cli/emit-attest.md)
- [`--emit-sandbox`](cli/emit-sandbox.md)
- [`--lockdown`](cli/lockdown.md)
- [Egress Allowlist (`allowedHosts`)](cli/allowed-hosts.md)
- [Per-Package Capabilities (`perry.permissions`)](cli/capabilities.md)
- [`perry audit --sbom`](cli/perry-audit-sbom.md)
- [Host Allowlist (nativeLibrary, compilePackages)](cli/allow-perry-features.md)
- [perry.toml Reference](cli/perry-toml.md)
- [Privacy & Telemetry](cli/telemetry.md)

---

# Internals

- [Memory Model](internals/memory-model.md)
- [Explicit Memory Control](internals/explicit-memory.md)

# Contributing

- [Architecture](contributing/architecture.md)
- [Building from Source](contributing/building.md)
- [Releasing Perry](contributing/releasing.md)
