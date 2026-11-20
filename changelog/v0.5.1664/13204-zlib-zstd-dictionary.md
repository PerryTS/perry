### Fixed

- Apply Zstd dictionaries in synchronous compression/decompression and stream constructors, preserving view bounds and reloading owned history after reset.
- Preserve backend Zstd error codes and add public parity coverage for direct and captured sync calls, sliced dictionaries, missing history, split-input reset streams, and no-dictionary controls.
