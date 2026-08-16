### Testing

- `tests/release/packages/next-app-route/fixture.sh` now runs the armed
  `routeModule.handle` bypass guard (its `perry-host.js` is byte-identical to
  `tests/fixtures/next-app-route/perry-host.js`) and greps every cold-start log
  for `generated handler bypassed` as a hard failure — the guard's only signal is
  the host log; `verify.mjs` exits 0 when it fires (#8161).
- The two hard-coded verifier passes per cold start became
  `PERRY_NEXT_ROUTE_VERIFIERS_PER_START` (default 10), so the default run is
  10 cold starts × 10 passes = 100 batches, matching
  `tests/test_next_app_route_dylib.sh` and #8040's 100-iteration bullet. Each
  10-pass process runs ~2-3 copying minors, so the default is sensitive to
  per-collection bugs and intermittently red on today's `main` (#8163: ~2% of
  post-minor batches lose one response); collection depth in one warm process
  is `PERRY_NEXT_ROUTE_WARM_PASSES` (#8215), a complementary knob. `=2`
  restores the previous coverage. The forced-evacuation arm is unchanged.
