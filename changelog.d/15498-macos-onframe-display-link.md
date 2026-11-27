`onFrame` on macOS fires on the display refresh instead of the 8 ms run-loop
pump, with the vsync time as a sub-millisecond timestamp on the
`performance.now()` clock (#15488). Frames tick from a `CADisplayLink` of the
main window, or of the main screen while no window is on a display, with a
`CVDisplayLink` fallback before macOS 14. The link pauses while no frame
callback is pending.
