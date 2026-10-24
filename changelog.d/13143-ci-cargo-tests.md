### Internal
- Repair compiler-test expectations and isolation, and bound runtime unit-test process lifetimes in CI.

Run zlib RSS witnesses in exact-test child processes so parallel libtest agents cannot contaminate process-wide memory samples. Keep operation counts, thresholds, and planted codec failures unchanged.

Isolate HTTP unit-test acceptance and completion counters to each test's worker thread, retaining exact request/socket assertions and process-wide production telemetry. Give the deeply nested IIFE fixture its established compilation stack while preserving its depth and time bound.
