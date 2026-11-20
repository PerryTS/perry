// Issue #1867 — perry/audio compile-only smoke test.
//
// Real playback needs a device with speakers, so this only verifies:
//   1. `import { ... } from "perry/audio"` resolves and lowers cleanly.
//   2. Every exported function dispatches to its perry_audio_* runtime
//      symbol (no "perry/audio: 'X' is not a known function" diagnostic).
//   3. The handle types round-trip through the f64 ABI.
//
// End-to-end audio is a separate manual step on a Mac (macOS is the only
// fully-implemented backend in this PR; iOS / tvOS / visionOS share the
// AVAudioEngine implementation; Linux / Windows / Android / watchOS are
// stubbed and ship in a follow-up PR backed by miniaudio).

import {
  loadSound,
  unload,
  play,
  stop,
  pause,
  resume,
  setVolume,
  setRate,
  setPan,
  fadeIn,
  fadeOut,
  crossfade,
  createBus,
  destroyBus,
  muteBus,
  soloBus,
  setMasterVolume,
  suspend,
  resumeAll,
  isPlaying,
  getDuration,
  getPosition,
  onEnded,
  onLoaded,
} from "perry/audio";

function main(): void {
  const sfx = createBus("sfx");
  const music = createBus("music", 0);

  const click = loadSound("assets/click.wav", sfx, false);
  const bgm = loadSound("assets/bgm.mp3", music, true);

  // Pre-load callbacks
  onLoaded(click, () => {
    console.log("click loaded");
  });

  // Overlapping plays of the same sound
  const id1 = play(click, 0.8, false, 1.0, 0.0, 0);
  const id2 = play(click, 0.5, false, 1.1, -0.3, 50);
  const bgmId = play(bgm, 1.0, true, 1.0, 0.0, 1000);

  onEnded(id1, () => {
    console.log("voice 1 ended");
  });

  // Per-voice tuning
  setRate(id2, 1.2);
  setPan(id2, 0.5);
  setVolume(id1, 0.9, 100);

  // Fades
  fadeIn(bgmId, 500, 1.0);
  fadeOut(id2, 200);

  // Bus controls
  setVolume(music, 0.3, 250);
  muteBus(sfx, true);
  soloBus(music, false);

  // Crossfade primitive
  const bgm2 = loadSound("assets/bgm_calm.mp3", music, true);
  const bgm2Id = play(bgm2, 0.0, true, 1.0, 0.0, 0);
  crossfade(bgmId, bgm2Id, 2000);

  // Pause/resume single voice
  pause(id1);
  resume(id1);

  // Master + introspection
  setMasterVolume(0.6, 500);
  const playing = isPlaying(id1);
  const dur = getDuration(click);
  const pos = getPosition(bgmId);
  console.log("playing:", playing, "duration:", dur, "position:", pos);

  // Lifecycle teardown
  stop(id1, 0);
  stop(click, 100);
  unload(click);
  destroyBus(sfx);

  // App-level pause / resume
  suspend();
  resumeAll();
}

main();
