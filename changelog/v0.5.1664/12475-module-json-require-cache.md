Stop rewriting a resolved `require("./data.json")` call into a static default import when the call
reaches through a `createRequire`-derived alias. That shortcut exists for the bare ambient
`require` in unwrapped ESM, which has no module-relative base (#10758); a `createRequire` alias
already carries one, so the rewrite only bypassed the runtime CommonJS loader — the sole owner of
`require.cache`. `require.cache[resolved]` stayed undefined (the fixture's own `cache!.exports`
read threw `TypeError`), and the loader's re-parse-on-cache-delete path, provided by a JSON
module's synthesized `__perry_cjs_factory`, was skipped along with it. Alias calls now take the
same side-effect-import-plus-runtime-call route `.cjs` sources already use, so the JSON stays in
the AOT graph and remains embedded for relocation while the runtime loader owns the cache.

Covers `node-suite/module/imports/json-require-identity` and
`node-suite/module/require/json-cache` in the faithful lane, plus `json-attribute` and
`cache-identity-deletion`. A unit test pins the alias path against the #10758 replacement.
