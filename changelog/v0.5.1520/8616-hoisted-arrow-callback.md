### Performance

- Resolve eligible immutable arrow callback parameters once at instance-method
  entry, then call the resolved target directly while retaining the complete
  closure dispatcher as a fail-closed fallback for ordinary, bound, rest,
  padded, reassigned, and otherwise unproven calls.

- On the unchanged `codehz/ecs` 10,000-entity query, a 9-pair exact-parent A/B
  reduced the read-only iteration median by 13.53% and accumulation by 16.19%,
  with 9/9 wins for both workloads, identical expected output, and identical
  15,775,976-byte executables.
