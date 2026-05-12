# Audio Capture

The `perry/system` module provides real-time audio capture from the device
microphone, with A-weighted dB(A) level metering and waveform sampling —
everything needed to build a sound meter, audio visualizer, or voice-level
indicator. Every snippet below is excerpted from
[`docs/examples/system/snippets.ts`](../../examples/system/snippets.ts) — CI
links it on every PR.

```typescript,no-test
{{#include ../../examples/system/snippets.ts:audio}}
```

## API Reference

### `audioStart()`

Start capturing audio from the device microphone. Returns `1` on success, `0`
on failure (permission denied, no microphone, etc.).

On platforms that require permission (iOS, Android, Web), the system
permission dialog is shown automatically.

### `audioStop()`

Stop audio capture and release the microphone.

### `audioGetLevel()`

Get the current A-weighted sound level (a smoothed value with a 125 ms time
constant). Typical ranges:

- ~30 dB — quiet room
- ~50 dB — normal conversation
- ~70 dB — busy street
- ~90 dB — loud music
- ~110+ dB — dangerously loud

### `audioGetPeak()`

Get the current peak sample amplitude (`0.0`–`1.0`). Useful for simple level
indicators without dB conversion.

### `audioGetWaveform(sampleCount)`

Get recent waveform samples for visualization. Pass the number of samples you
want; the runtime returns the most recent N readings from its internal ring
buffer. Useful for drawing waveform displays or level history charts.

### `audioSetOutputFilename(filename)`

Set the destination path for the next call to `audioStartRecording`. Pass an
absolute path or a path relative to the app's working directory. Must be
called **before** `audioStartRecording`.

### `audioStartRecording()`

Begin writing captured microphone audio to the file set by
`audioSetOutputFilename`. The output is a WAV file (16-bit PCM, mono,
48 kHz on every platform). Calling without a destination set is a no-op.

### `audioStopRecording()`

Finalize the in-progress recording — flushes pending samples, writes the
RIFF/WAVE header sizes, and closes the file. Safe to call when no
recording is in flight.

```typescript,no-test
import {
  audioStart,
  audioStop,
  audioSetOutputFilename,
  audioStartRecording,
  audioStopRecording,
} from "perry/system";

audioStart();
audioSetOutputFilename("/tmp/captured.wav");
audioStartRecording();
// … capture for some duration …
audioStopRecording();
audioStop();
```

`audioStartRecording` does not imply `audioStart` — start the input first,
then start the file writer.

## Platform Implementations

| Platform | Audio Backend | Permissions |
|----------|--------------|-------------|
| macOS | AVAudioEngine | Microphone permission dialog |
| iOS | AVAudioSession + AVAudioEngine | System permission dialog |
| Android | AudioRecord (JNI) | RECORD_AUDIO permission |
| Linux | PulseAudio (libpulse-simple) | None (system-level) |
| Windows | WASAPI (shared mode) | None |
| Web | getUserMedia + AnalyserNode | Browser permission dialog |

All platforms capture at 48 kHz mono and apply the same A-weighting filter
(IEC 61672 standard, 3 cascaded biquad sections).

## Next Steps

- [Camera](../ui/camera.md) — Live camera preview (iOS)
- [Overview](overview.md) — All system APIs
