- **`npm pack --json` changed shape at npm 12; the publish parser now accepts
  both.** Release run 34335433079 failed with
  `npm pack failed for @perryts/perry-darwin-arm64@0.5.1519` **after all fourteen
  build legs had gone green** — the furthest any attempt had reached. The pack
  itself succeeded (exit 0, tarball written); only the parse failed:

  ```
  npm 11:  [ { "filename": …, "shasum": … } ]                  ← array
  npm 12:  { "@perryts/perry-darwin-arm64": { "filename": … } } ← object, keyed
  ```

  `packTarball` did `Array.isArray(parsed) ? parsed[0] : undefined`, so npm 12
  yielded `undefined` and the caller reported a pack failure that never happened.
  It now accepts both shapes, and logs the raw payload when it cannot — the
  original code discarded it, which is why a one-line shape change cost a full
  release cycle to identify.

  Verified against the exact npm CI installs (12.0.2): the old parser returns
  `undefined`, the new one packs successfully; npm 11.19.1 still passes.

- **The publish job pins `npm@11` instead of `npm@latest`.** The step exists to
  clear the 11.5.1 OIDC floor, but `@latest` silently opted the repo's most
  privileged job (`id-token: write`) into every future npm major — and npm 12.0.2
  duly broke it. `@11` clears the floor by a wide margin. Moving to a new major
  is now a deliberate act, with a note to re-check `proof.mts` against that
  major's `pack --json` output.
