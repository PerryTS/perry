Fixed a `perry-hir` unit test left stale by the native-binding removal in
v0.5.1610. `eval_classifier::tests::remedy_is_scoped_to_bundled_npm_shims`
asserted `shimmed_package_module("dayjs.businessDaysAdd") == Some("dayjs")`,
but `dayjs` was dropped from `NATIVE_MODULES` along with `qs`, `fastify`,
`date-fns`, `rate-limiter-flexible` and `node-cron`, so the classifier
correctly answered `None`. The production code was right; the assertion had
outlived its subject, and because per-PR `cargo-test` is scoped to the crates a
diff touches, the red would have landed on the next unrelated PR anywhere in
`perry-hir`'s reverse-dependency closure.

Rather than swap in another package name — `lru-cache`, `commander`, `pg`,
`mysql2` and `decimal.js` are queued for the same removal — the positive cases
now derive from the live registry: a test helper filters `NATIVE_MODULES` down
to the entries that are neither Node builtins nor Perry-owned surfaces, and
asserts every one of them resolves and names itself in the `compilePackages`
remedy. A binding removal now shrinks that set instead of reddening the test.
The sibling test that asserted the end-to-end remedy message was carrying the
same trap on `lodash` and was derived the same way.

The negative half stays literal on purpose: `fs.bogus`, `node:fs.bogus`,
`perry/gc.notReal`, `crypto.subtle.digest`, `some-random-package.thing` and a
bare word are *categories*, each pinning a distinct branch of
`shimmed_package_module`, and none of them can be deleted by a removal PR.
Together the two halves keep the test able to fail in both directions — a
`shimmed_package_module` stubbed to always return `None` fails the derived
positives, and one stubbed to always return `Some` fails the literal negatives.
Both were confirmed by sabotage before landing. If the removal campaign ever
empties the shim set, the helper's emptiness assertion fires with a message
saying the remedy has become dead code and should be deleted outright.
