### Fixed

- Native UI and audio backends now copy runtime strings into owned Rust
  storage before use, preventing dangling borrows when Perry's garbage
  collector relocates the original string.
