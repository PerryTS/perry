### Testing

- Add the pinned Next.js 16.3.0 / React 19.2.4 production App Route fixture and
  its 21-request verifier as real, checked-in files (`tests/fixtures/next-app-route`),
  plus a Node oracle test that rebuilds the app from its lockfile and proves the
  generated bundle exports `AppRouteRouteModule.handle` for `/api/benchmark`
  before any Perry-specific assertion runs (#8034).

- Add `tests/test_next_app_route_dylib.sh`: it compiles that unmodified
  production route to an **app-only dylib**, links it against separately built
  runtime and stdlib/HTTP provider images, checks every undefined `js_`/`perry_`
  symbol the app needs is exported by those providers, and then drives 10 cold
  starts x 10 verifier repetitions (the 100 repetitions #8037 asks for).

  The host installs a wrapper around `routeModule.handle` and fails any request
  that reaches the generated handler without passing through it, so the route
  cannot be "passed" by a compatibility path that calls the userland `GET`
  directly. Because that assertion runs inside a `.then()` after the response is
  already sent, the verifier's exit code cannot carry it — the gate greps the
  host log for the diagnostic instead.

- Add the `Next App Route dylib` workflow on `workflow_dispatch` + a nightly
  schedule. It is deliberately **not** a per-PR required gate yet: the graph is
  104 modules, one generated chunk has taken ~112 min in a single codegen unit,
  and the route still needs the computed relative chunk require from #8146.
