// Issue #5927 — same as test_issue_5927_order_a.ts but with the import
// statements REVERSED (namespace first, plain import second), verifying the
// fix is order-independent: `.or_insert()` on the namespace side means the
// namespace claims the (empty) slot first here, but the plain import's
// later UNCONDITIONAL `.insert()` still overwrites it.
import { NsA } from "./fixtures/issue_5927_pkg/barrel.ts"
import { omit } from "./fixtures/issue_5927_pkg/plain_mod.ts"

console.log(omit())
console.log(NsA.omit())
