Fix Android dark-mode changes recreating `PerryActivity` and rerunning the compiled app's `main()` in the same process. The Android template now handles `uiMode` configuration changes while preserving the existing Activity and view state.

Add an explicit-device emulator regression using the production Activity and a JNI startup fixture. It reproduces duplicate startup on the old manifest and verifies a single startup, retained counter/view identity, and working callbacks across three night-mode changes with the fix. No version bump.
