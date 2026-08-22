### Fixed

- Preserve Fetch `Request`, `Headers`, and `FormData` registry roots for every
  application thread in an embedded multi-app host. Previously a
  process-global registration latch could install the Fetch scanner only for
  the first Perry heap, so a second in-process Next application could reject
  its HTTP handler and return 500. Add a two-application-thread regression test
  for the per-thread scanner contract. (#8546)
