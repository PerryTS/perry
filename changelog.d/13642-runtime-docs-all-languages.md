Refresh the runtime and compiler documentation against the current implementation.

- Describe copying and non-moving collection, target-specific precise roots,
  young non-moving large-object storage, inline births, native handles, weak
  processing, and the distinction between JavaScript GC and Rust ownership.
- Explain per-module program-facts solving, cross-module contracts, field/call
  specialization, guarded fallback, proof diagnostics, and generic versus native
  representations. Correct stale eval, CommonJS, Node-API and threading claims.
- Translate the revised passages into all ten published languages and regenerate
  gettext catalogs with the pinned toolchain. Keep section anchors stable across
  translations. Existing unrelated translation gaps retain current-English fallback.

Validation: English and all ten localized mdBook builds; gettext syntax/format
checks; translated-passage coverage and code/link preservation; catalog extraction
and sync reproducibility; GC claim/knob checks and i18n toolchain tests. This is a
documentation-only change; no compiler or runtime behavior changes.
