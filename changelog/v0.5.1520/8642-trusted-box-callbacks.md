### Performance

- Validate the exact capture layout and live compiler-owned box slots of eligible
  direct arrow callbacks once at method entry, then use a bounded private callback
  body for captured-box access while retaining the fully checked public body as a
  fail-closed fallback.

- On the unchanged `codehz/ecs` 10,000-entity query, an 11-pair exact-parent A/B
  reduced median paired read-only iteration time by 12.94% and accumulation by
  13.69%, with 11/11 wins for both workloads and every correctness oracle passing.
  The benchmark executable grew by 16,544 bytes (0.105%).
