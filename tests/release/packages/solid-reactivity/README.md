# Solid client-runtime audit

This is the first prerequisite for the native UI bridge in [#4644](https://github.com/PerryTS/perry/issues/4644).
It compiles the installed, unmodified Solid 1.9.15 core, store, and universal
renderer to native code and compares their output with Node. No display server
or JavaScript runtime is needed by the resulting binary.

```sh
PERRY_BIN=/absolute/path/to/perry \
  tests/release/packages/_harness.sh --filter solid-reactivity
```

Use the repository's `.node-version` for the oracle. The release sweep's
package tier discovers this fixture automatically. Its lockfile pins Solid
and its transitive dependencies; `fixture.sh` uses `npm ci` on a fresh checkout.

The fixture exercises:

- Signals, memo invalidation, batched updates, equality suppression, and disposal.
- Dynamic effect dependencies and cleanup before reruns and disposal.
- Store proxies, nested reads, updater functions, `produce`, keyed `reconcile`,
  preserved item identity, and `unwrap`.
- The real universal renderer over an in-memory host: keyed insertion,
  anchored reordering, replacement, removal, owner cleanup, and text updates
  that preserve node identity.

## Selecting the reactive build

Solid's `node` export selects its server implementation. That implementation
does not subscribe effects to updates. Perry's normal Node-compatible package
resolution therefore needs these existing project aliases for a reactive app:

```json
{
  "perry": {
    "compilePackages": ["solid-js"],
    "allow": { "compilePackages": ["solid-js"] },
    "packageAliases": {
      "solid-js": "solid-js/dist/solid.js",
      "solid-js/store": "solid-js/store/dist/store.js"
    }
  }
}
```

The core alias also applies to imports inside the store and universal renderer,
so they share the same owner and dependency state. Importing the client core
only in the app would leave those internal imports pointing at the server
build. The Node oracle uses `--conditions=browser` to select the corresponding
client exports.

This fixture does not establish a `perry/ui` adapter, native event handling,
or a Solid JSX compiler mode. It also does not audit resources, transitions,
hydration, or every store operation. Solid's bundled `h` and `html` entry
points use its web renderer; they are not a native hyperscript API. See
[Solid's universal-renderer contract](https://github.com/solidjs/solid/blob/main/packages/solid/universal/README.md)
for the host operations and the separate universal JSX transform.

## Moving-GC verification finding

On main `d36a1af0c`, the normal fixture matches the Node oracle. A forced
copying run with seed 4644 and protected from-space also matches (22 copying
minors, 14,022 moved objects on macOS arm64). Enabling the evacuation verifier
exposes a failure during store creation:

```sh
PERRY_GC_SCHEDULE_SEED=4644 PERRY_GC_SCHEDULE_RATE=1 \
PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_VERIFY_EVACUATION=1 \
  tests/release/packages/solid-reactivity/out
```

The verifier reports `stale forwarded pointer in remembered dirty ranges` at
the fifth scheduled collection. A store-only reduction also reproduces a
stale pointer in `heap fields`. The signal-only probe passes verification.
This finding needs resolution before declaring the native bridge's GC audit
complete; normal output parity alone does not resolve it.

Further isolation identified the reported field as the effect computation's
`sources` array after growing from capacity 8 to 16. Both the old forwarding
stub and its target are tenured. Array growth deliberately retains such
aliases for `clean_arr_ptr` to follow, whereas the verifier currently rejects
any forwarded reference. The follow-up needs to distinguish those retained
growth aliases from evacuation originals that are about to be reclaimed.
