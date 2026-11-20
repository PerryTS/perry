### Fixed

- Isolate native-async registry state by token creator in Rust test builds so
  parallel microtask pumps cannot drain another test's completion, eliminating
  the wrong-thread rejection flake from #8435.
