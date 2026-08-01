# Zero-config bindings and faithfulness

This note describes Perry's default routing for installed npm dependencies and
the compatibility marker used by bundled `perry-ext-*` bindings.

## Resolution behavior

For a bare import, Perry first checks whether the module is in its native
module manifest. Unless the root package was explicitly selected through
`perry.compilePackages`, a native module is served by its bundled binding and
file resolution does not walk into the installed package source.

Other reachable packages are AOT-compiled by default. When the host
`package.json` omits `perry.compilePackages`, Perry enumerates installed
packages, skips natively shimmed packages, and routes the remaining package
source through the compiler. This is equivalent to:

```json
{
  "perry": {
    "compilePackages": "auto"
  }
}
```

`"all"`, `true`, and a literal `"*"` array entry have the same universal
routing meaning. An explicit package list constrains routing; `false` or `[]`
opts out entirely and restores the listed-only V8-free gate.

When no allow policy is present, universal auto routing also receives the
universal compile allow. Explicit trust policy is never discarded:

- `perry.allow.compilePackages` can constrain the packages admitted by auto
  routing;
- `perry.allow.compilePackages: false` or `[]` fails closed;
- `PERRY_ALLOW_PERRY_FEATURES=0` clears the allowlist and fails closed;
- `PERRY_ALLOW_PERRY_FEATURES=1` remains the one-off universal override.

## The compatibility marker

Each row in `crates/perry/well_known_bindings.toml` may declare:

```toml
[bindings.example]
crate = "perry-ext-example"
lib = "perry_ext_example"
compat = "partial"
```

The two values are:

- `full`: an exhaustively audited drop-in for the pinned npm package's public
  API and observable behavior;
- `partial`: a subset or a wrapper that has not yet passed that audit.

An absent or unknown value is `partial`. Aliases inherit the target binding's
effective marker; missing targets and alias cycles fail closed as partial.

The current third-party wrappers remain partial. Several superficially small
wrappers still differ materially from their pinned packages: the UUID wrapper,
for example, lacks exports including `parse` and `stringify`; slugify lacks
`remove`, `locale`, the complete character map, and `extend`; nanoid flattens a
curried API and omits exports; and dotenv implements only part of its current
surface. A `full` marker should be added only after the implementation and
conformance tests cover the complete pinned surface.

## Diagnostics and strict mode

When a partial binding wins while a copy of its root package exists in
`node_modules`, text-mode builds emit one informational note per package. The
note points to `perry.compilePackages`, which makes the installed JavaScript
source win instead.

For CI that must never substitute a partial wrapper, set:

```sh
PERRY_REQUIRE_FAITHFUL_BINDINGS=1 perry compile src/main.ts
```

Only enabled values (`1` or `true`, case-insensitive) activate strict mode.
Under strict mode Perry refuses the partial auto-preference and identifies the
binding and importing module. Add the root package to both
`perry.compilePackages` and the applicable allow policy to compile the real
source, or disable strict mode to accept the bundled subset.

Registered subpaths are classified before root fallback. Thus an import such
as `mysql2/promise` uses that alias row and inherits `mysql2`'s compatibility,
while the installed-copy probe and `compilePackages` suggestion correctly use
the root package name `mysql2`.

Both `PERRY_REQUIRE_FAITHFUL_BINDINGS` and `PERRY_ALLOW_PERRY_FEATURES` are
included in the build-cache environment fingerprint, so changing either policy
cannot reuse an artifact produced under different routing rules.
