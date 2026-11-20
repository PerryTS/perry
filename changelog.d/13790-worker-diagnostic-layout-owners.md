### Fixed

- Declare Worker ownership of the shared event-loop-utilization and CPU-usage result layouts, preventing sparse-realm birth aborts without retaining unrelated perf-hooks or process records.
