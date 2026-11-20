Fix Android `wheelPickerSetSelected` and `wheelPickerGetSelected` inside UI callbacks by sharing item metadata with the native startup thread. UI callbacks can also append items without losing the original list. The metadata lock is released before calling JNI.

Device validation reproduces the old callback result of `-1` after native-thread selection of `5`. With the fix, a UI button callback selects and reads `2`, then appends and selects item `10`; invalid selection and handle behavior is preserved. No version bump.
