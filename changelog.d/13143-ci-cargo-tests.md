### Internal
- Repair compiler-test expectations and isolation, and bound runtime unit-test process lifetimes in CI.

Run zlib RSS witnesses in exact-test child processes so parallel libtest agents cannot contaminate process-wide memory samples. Keep operation counts, thresholds, and planted codec failures unchanged.
