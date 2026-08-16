# Production Next App Route fixture

This is the pinned fixture from #8034. Its application and verifier sources are
copied verbatim from the issue:

- Next.js 16.3.0, built with `next build --webpack`
- React and React DOM 19.2.4
- twenty concurrent GET requests plus one POST request
- the generated `routeModule.handle` / `AppRouteRouteModule.handle` path
- request-local `headers()` reads across a dynamic import and a timer
- a two-chunk streamed `NextResponse` with status, header, and cookie checks

`package-lock.json` is committed so a gate cannot silently move the framework
graph. Generated `.next/` output and `node_modules/` are deliberately ignored.

Run the Node oracle and generated-route structural check with:

```sh
tests/test_next_app_route_node_oracle.sh
```

The Node check is only the oracle half of #8034. It does not claim Perry
acceptance. The Perry half compiles the production webpack output as an
app-only dylib, loads separate runtime and stdlib provider images before the app
with eager relocation, and builds both providers from one unified Cargo graph
so all runtime bindings share one image. It enters the generated route-module
handle path and runs the same `verify.mjs` without a direct `GET` call or
fabricated response.

Run that integration gate with:

```sh
PERRY_BIN=target/perry-dev/perry tests/test_next_app_route_dylib.sh
```

By default it performs ten cold provider-host starts and ten verifier passes
per start (100 total). `PERRY_NEXT_COLD_STARTS` and
`PERRY_NEXT_VERIFICATIONS_PER_START` can reduce the count for local iteration.
