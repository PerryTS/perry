# Bundled native-binding governance

Perry's compatibility goal is to compile real TypeScript and JavaScript
packages. A package-specific Rust rewrite can be useful as a temporary bridge,
but it is not the default compatibility strategy and does not demonstrate that
Perry can compile the package it replaces.

Applications are expected to install their declared dependencies with npm,
Bun, pnpm, or another package manager. Source-package migrations therefore do
not preserve a zero-install fallback: after a bundled rewrite is removed, the
import resolves from the application's installed dependency graph like any
other npm package.

## Policy

Use this order when adding package compatibility:

1. Compile the upstream JavaScript or TypeScript package source with
   `perry.compilePackages`.
2. When that fails on a reusable Node.js or Web API, improve the shared
   runtime API rather than rewriting the package.
3. Use `perry.nativeLibrary` only for a true native-addon or system-library
   boundary. Domain-specific bindings should ship as independently versioned
   external packages.
4. Keep an in-tree `perry-ext-*` crate only for a shared runtime API or a small
   strategic shim whose maintenance and release cost has been accepted
   explicitly.

A proposal for a new bundled extension must identify the native boundary or
shared runtime capability, explain why compiling the package source is not
viable, name an owner, account for build and binary cost, define its
faithfulness level, and include an exit or consolidation plan. An ordinary npm
package is not eligible merely because a Rust implementation is convenient.

## Categories and migration gates

- **Runtime API** bindings implement foundational Node.js or Web capabilities
  used by many packages. They stay in or near core for now and may eventually
  be consolidated into `perry-stdlib` or the runtime. A third-party package
  alias can still be reassessed separately.
- **Source package** bindings replace packages whose upstream implementation
  can in principle be compiled. Their migration target is the real upstream
  source plus fixes to shared language, module, and runtime support.
- **External integration** bindings cross a real native boundary or provide a
  product/domain integration. Their migration target is an official or
  third-party package using `perry.nativeLibrary`, with its own release cycle.
- **Obsolete integration** bindings have no supported long-term owner. Remove
  them only after checking current consumers and documenting the compatibility
  impact.

`perry-ext-fetch` is a mixed case: compiling `node-fetch` upstream does not
remove the Fetch API. The shared `fetch`/`Headers`/`Request`/`Response`
capability must first live in the runtime; only the package-specific alias and
wrapper are migration candidates.

Migration candidates remain bundled for compatibility. Remove a source-package
binding only after a representative upstream version compiles, has conformance
coverage for the supported surface, and has an upgrade note. Remove an external
integration only after installable artifacts exist for the supported targets
and the replacement path is documented. Removing a well-known mapping before
those gates pass is a breaking change.

The root workspace's `default-members` intentionally excludes extension
crates. Distribution builds use the inventory below as their explicit shipping
set; changing that set therefore requires changing the recorded decision and
passing the governance check.

## Completed source migrations

- `slugify@1.6.9` compiles from its installed CommonJS source through the
  default automatic package-routing path. The #5716 E2E test compares Perry
  with Node across transliteration, replacement, strict/trim options, locale,
  regular-expression removal, and `slugify.extend`. Its former
  `perry-ext-slugify` and `perry-stdlib` implementations have been removed.

## Current inventory

`workspace-architecture.json` is the source of truth for the crate decision and
binding-specific migration target. The CI check requires it to cover every
`perry-ext-*` directory and workspace member, and joins it with package mappings
from `well_known_bindings.toml`. Regenerate this table with
`python3 scripts/binding_governance.py --table`.

<!-- BEGIN GENERATED BINDING GOVERNANCE -->
| Crate | Package mapping(s) | Category | Migration target | Current status |
|---|---|---|---|---|
| `perry-ext-ads` | `perry/ads` | Obsolete integration | Remove after compatibility review | Bundled; removal pending |
| `perry-ext-argon2` | `argon2` | External integration | Move to an external native package | Bundled; migration pending |
| `perry-ext-axios` | `axios` | Source package | Compile the upstream package source | Bundled; migration pending |
| `perry-ext-bcrypt` | `bcrypt` | External integration | Move to an external native package | Bundled; migration pending |
| `perry-ext-better-sqlite3` | `better-sqlite3` | External integration | Move to an external native package | Bundled; migration pending |
| `perry-ext-cheerio` | `cheerio` | Source package | Compile the upstream package source | Bundled; migration pending |
| `perry-ext-commander` | `commander` | Source package | Compile the upstream package source | Bundled; migration pending |
| `perry-ext-cron` | `cron`<br>`node-cron` | Source package | Compile the upstream package source | Bundled; migration pending |
| `perry-ext-dayjs` | `date-fns`<br>`dayjs` | Source package | Compile the upstream package source | Bundled; migration pending |
| `perry-ext-decimal` | `bignumber.js`<br>`decimal.js` | Source package | Compile the upstream package source | Bundled; migration pending |
| `perry-ext-dotenv` | `dotenv` | Source package | Compile the upstream package source | Bundled; migration pending |
| `perry-ext-ethers` | `ethers` | Source package | Compile the upstream package source | Bundled; migration pending |
| `perry-ext-events` | `events` | Runtime API | Keep near core; consolidate when practical | Bundled; retained |
| `perry-ext-exponential-backoff` | `exponential-backoff` | Source package | Compile the upstream package source | Bundled; migration pending |
| `perry-ext-fastify` | `fastify` | Source package | Compile the upstream package source | Bundled; migration pending |
| `perry-ext-fetch` | `fetch`<br>`node-fetch` | Source package | Compile the upstream package source | Bundled; migration pending |
| `perry-ext-http` | `http`<br>`http2`<br>`https` | Runtime API | Keep near core; consolidate when practical | Bundled; retained |
| `perry-ext-ioredis` | `ioredis`<br>`iovalkey`<br>`redis` | Source package | Compile the upstream package source | Bundled; migration pending |
| `perry-ext-jsonwebtoken` | `jsonwebtoken` | Source package | Compile the upstream package source | Bundled; migration pending |
| `perry-ext-lru-cache` | `lru-cache` | Source package | Compile the upstream package source | Bundled; migration pending |
| `perry-ext-moment` | `moment` | Source package | Compile the upstream package source | Bundled; migration pending |
| `perry-ext-mongodb` | `mongodb` | Source package | Compile the upstream package source | Bundled; migration pending |
| `perry-ext-mysql2` | `mysql2`<br>`mysql2/promise` | Source package | Compile the upstream package source | Bundled; migration pending |
| `perry-ext-nanoid` | `nanoid` | Source package | Compile the upstream package source | Bundled; migration pending |
| `perry-ext-net` | `net` | Runtime API | Keep near core; consolidate when practical | Bundled; retained |
| `perry-ext-node-forge` | `node-forge` | Source package | Compile the upstream package source | Bundled; migration pending |
| `perry-ext-nodemailer` | `nodemailer` | Source package | Compile the upstream package source | Bundled; migration pending |
| `perry-ext-parcel-watcher` | `@parcel/watcher`<br>`@parcel/watcher-darwin-arm64`<br>`@parcel/watcher-darwin-x64`<br>`@parcel/watcher-linux-arm64-glibc`<br>`@parcel/watcher-linux-arm64-musl`<br>`@parcel/watcher-linux-x64-glibc`<br>`@parcel/watcher-linux-x64-musl`<br>`@parcel/watcher-win32-arm64`<br>`@parcel/watcher-win32-x64` | External integration | Move to an external native package | Bundled; migration pending |
| `perry-ext-pdf` | `@perryts/pdf` | External integration | Move to an external native package | Bundled; migration pending |
| `perry-ext-pg` | `pg` | Source package | Compile the upstream package source | Bundled; migration pending |
| `perry-ext-qs` | `qs` | Source package | Compile the upstream package source | Bundled; migration pending |
| `perry-ext-ratelimit` | `rate-limiter-flexible` | Source package | Compile the upstream package source | Bundled; migration pending |
| `perry-ext-sharp` | `sharp` | External integration | Move to an external native package | Bundled; migration pending |
| `perry-ext-streams` | `streams` | Runtime API | Keep near core; consolidate when practical | Bundled; retained |
| `perry-ext-typescript` | `typescript` | Source package | Compile the upstream package source | Bundled; migration pending |
| `perry-ext-undici` | `undici` | Source package | Compile the upstream package source | Bundled; migration pending |
| `perry-ext-uuid` | `uuid` | Source package | Compile the upstream package source | Bundled; migration pending |
| `perry-ext-validator` | `validator` | Source package | Compile the upstream package source | Bundled; migration pending |
| `perry-ext-ws` | `ws` | Runtime API | Keep near core; consolidate when practical | Bundled; retained |
| `perry-ext-zlib` | `zlib` | Runtime API | Keep near core; consolidate when practical | Bundled; retained |
<!-- END GENERATED BINDING GOVERNANCE -->
