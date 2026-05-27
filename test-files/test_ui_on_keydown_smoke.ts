// Issue #1864 smoke test: onKeyDown / onKeyUp / focus / blur / isKeyDown
// link cleanly through the perry/ui FFI surface on `--target macos`.
//
// We don't synthesize key events (no test harness for that yet) — this
// fixture just verifies the dispatch table + FFI bindings exist and that
// the binary exits cleanly under PERRY_UI_TEST_MODE=1.

import {
    App, VStack, Text,
    onAppKeyDown, onAppKeyUp, isKeyDown, currentModifiers,
    Key, Modifier,
} from "perry/ui";

let pressed = 0;

App({
    title: "Keyboard Smoke",
    width: 320,
    height: 200,
    body: VStack([
        Text("Hold a key.", "label"),
    ]),
});

// App-level handler — fires when no widget owns focus. The `key` argument is
// the numeric `Key` enum, so comparisons compile to integer equality.
onAppKeyDown((key: Key, mods: number, repeat: boolean) => {
    if (!repeat) pressed++;
    if (key === Key.Space) console.log("space down, mods=", mods);
    if (key === Key.ArrowLeft && (mods & Modifier.Shift)) console.log("shift+left");
});

onAppKeyUp((key: Key, _mods: number) => {
    if (key === Key.Escape) console.log("escape up");
});

// Branchless polls — always 0 at compile time since no event was synthesized.
if (isKeyDown(Key.Space)) console.log("space held");
if (isKeyDown(Key.Numpad7)) console.log("numpad 7 held");
if (isKeyDown(Key.F13)) console.log("f13 held");

// Modifier polling outside an event handler — typical "snap-to-grid while
// Shift is held" idiom during mouse drag.
if (currentModifiers() & Modifier.Shift) {
    console.log("shift currently held");
}
