Android `perry/ui` apps that link perry-stdlib no longer abort at launch or exit
on the first re-rendering callback. The stdlib's bundled copy of the `jni` crate
(reached through hickory-proto) is localized at link time, so
perry-ui-android's own copy is the one its JNI calls bind to — previously
`AttachGuard::from_unowned` bound to the stdlib copy and jni's frame-level check
aborted in `PerryBridge.nativeInit`. The compiled `main()` now runs on the
Activity's UI thread, the thread every callback, timer pump tick and GC step
already used, instead of a parked `perry-native` thread whose per-thread runtime
state the callbacks never saw (empty strings, `TypeError: Invalid native
property owner`, then the FORTIFY destroyed-mutex abort in `hwuiTask`); the
event loop is flagged host-driven so `main()` returns to the Looper with live
timers. Also on Android: BottomNavigation highlights the tapped tab,
`preferencesGet` returns `undefined` for an absent key, and borderless buttons
no longer keep Material's 88dp minimum width.
