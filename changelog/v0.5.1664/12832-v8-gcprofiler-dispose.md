### Fix inherited GCProfiler disposal

`v8.GCProfiler` instances now inherit a shared `Symbol.dispose` method from their constructor prototype. The missing prototype link and disposer previously left disposal unavailable. The method calls the actual receiver's `stop()` once, preserves overrides and thrown errors, and returns `undefined`, matching Node. Existing profiler state and reset behavior are reused.
