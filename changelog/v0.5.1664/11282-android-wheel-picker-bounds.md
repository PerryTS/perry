Keep Android `WheelPicker` selection bounded by the first and last items instead of enabling wrap-around for longer lists. The wrapping policy is reapplied when items are added, matching iOS behavior.

Validated on an Android API 35 arm64 emulator: the original ten-item picker wrapped backward from 0 to 9; with the fix, both endpoints remain bounded for 1, 2, 3 and 10 items while interior scrolling still advances selection. No version bump.
