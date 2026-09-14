Add opt-in `App({ frameAutosaveName: "main", ... })` desktop window persistence
(#10170). macOS uses AppKit frame autosave; Windows and WinUI restore native
placement and maximized/fullscreen state; GTK4 restores normal size and window
state while leaving positioning to the compositor. Saved frames take precedence
over launch defaults, and empty or omitted names disable persistence.

Persistence keys are scoped to the executable and window name. Windows/Linux
settings are validated and replaced atomically, minimized Windows sessions reopen
in their last non-minimized state, and fullscreen placement preserves the normal
frame. Add compiler regression coverage, storage tests, a native Windows
close/reopen test, TypeScript declarations, and a desktop example.
