### Fixed

- **`dotenv.parse` no longer compiles to a swallowed runtime throw.** The
  native implementation (`js_dotenv_parse`) has shipped since the dotenv module
  was added, and codegen already declared the extern — but the API manifest
  only ever registered `dotenv.config`, and `NATIVE_MODULE_TABLE` only ever had
  a `config` dispatch row. The #463 unimplemented-API gate therefore fired on
  every `dotenv.parse(...)` call site and, under the default defer policy,
  compiled it to a throw-on-reach runtime error.

  This is a data-loss-shaped bug rather than a missing feature. Config loaders
  wrap the parse in `try { … } catch {}` so a malformed `.env` isn't fatal
  (Socket Firewall's `readConfigFile()` is exactly this shape), which means the
  deferred throw landed in the `catch` and was **swallowed**: the program ran
  as though the `.env` file did not exist, with no crash, no warning, and no
  runtime notice. Every setting in the file was silently dropped.

  Fixed by adding the two missing rows — a `dotenv.parse` dispatch row
  (`args: &[NA_STR]`, `ret: NR_OBJ_FROM_JSON_STR`) and the matching
  `parse(src: string): any` manifest entry — plus regenerated
  `docs/api/perry.d.ts` / `docs/src/api/reference.md`.

  The return kind carries the fix as much as the row does: `js_dotenv_parse`
  returns a JSON *string*, so `NR_OBJ_FROM_JSON_STR` (the same kind
  `jsonwebtoken.decode` uses) pipes it through `js_json_parse` and hands
  TypeScript a real object. Wired as a plain `NR_STR` the call would have
  compiled and appeared to work while `parsed.FOO` read `undefined` — a quieter
  version of the same bug — so the regression test asserts the return kind, not
  just the row's existence.

  Both the default-import (`dotenv.parse(...)`) and named-import
  (`import { parse } from 'dotenv'`) forms now route to the native code and
  compile with zero deferred-site notices; empty input returns `{}`, not `null`.
  Covered by `dotenv_parse_is_registered` (perry-api-manifest) and
  `dotenv_parse_dispatches_to_native_impl_as_an_object` (perry-codegen), both
  `src/` unit tests so they run in the per-PR `cargo-test` gate.
