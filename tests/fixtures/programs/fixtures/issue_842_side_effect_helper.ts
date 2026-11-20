// Helper for test_issue_842_side_effect_dynamic_import.ts. NO `export`
// statements — purely side-effecting. Pre-#842 this module never emitted
// `@__perry_ns_<prefix>` (the producer-side short-circuit on
// `namespace_entries.is_empty()`), but the consumer module still
// declared the symbol extern, causing a link-time undefined-symbol
// error: `Undefined symbols ... ___perry_ns_...`.
console.log("helper-side-effect-ran");
