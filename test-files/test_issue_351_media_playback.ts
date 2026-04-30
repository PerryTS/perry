// Issue #351 — perry/media compile-only smoke test.
//
// Real audio playback needs a device with speakers and a network
// connection, so this only verifies that:
//   1. `import { ... } from "perry/media"` resolves and lowers cleanly.
//   2. Every exported function lowers to its codegen path (no
//      "perry/media: 'X' is not a known function" diagnostic).
//   3. The handle round-trips through createPlayer → numeric API.
//
// End-to-end audio verification is a separate manual step on a Mac (the
// macOS backend is the only one fully implemented in this PR; iOS / tvOS /
// visionOS share the same code with UIImage adaptations; Android / GTK4 /
// Windows are stubbed and will surface playback in follow-up PRs).

import {
  createPlayer,
  play,
  pause,
  stop,
  seek,
  setVolume,
  setRate,
  getCurrentTime,
  getDuration,
  getState,
  isPlaying,
  onStateChange,
  onTimeUpdate,
  setNowPlaying,
  destroy,
} from "perry/media";

// Use a known-stable URL — Apple's iTunes Radio sample MP3. The compile
// path lowers it to a string arg; the runtime call is fenced behind the
// platform-specific backend so on stub platforms `createPlayer` returns 0.
const url = "https://download.samplelib.com/mp3/sample-3s.mp3";
const player = createPlayer(url);
console.log("created player handle:", player);

if (player !== 0) {
  setVolume(player, 0.5);
  setRate(player, 1.0);

  onStateChange(player, (state) => {
    console.log("state:", state);
  });

  onTimeUpdate(player, (current, duration) => {
    // ~10 Hz; print is too chatty for a smoke test, just exercise the
    // closure-callback marshaling.
    if (current === 0 && duration === 0) {
      // suppress
    }
  });

  setNowPlaying(player, "Sample Title", "Sample Artist", "Sample Album", "");

  // Exercise every accessor without actually waiting for playback.
  console.log("state at create:", getState(player));
  console.log("isPlaying:", isPlaying(player));
  console.log("currentTime:", getCurrentTime(player));
  console.log("duration:", getDuration(player));

  // Exercise the control surface — these are no-ops on stub platforms.
  play(player);
  pause(player);
  seek(player, 1.5);
  stop(player);

  destroy(player);
  console.log("destroyed");
} else {
  console.log("createPlayer returned 0 (stub backend or invalid URL)");
}

console.log("smoke OK");
