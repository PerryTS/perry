### Fixed

- A program that allocates and frees short-lived native scratch in a loop — any `RegExp` call does — no longer accumulates false collection pressure. A million-call `.test()` loop was paying three old-generation collection cycles, one of them a full that traced a 52 MB live heap to free 59 KB, costing 33% more instructions per call (#10376).
