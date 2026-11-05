**`--opt-report` names where a local lives (#7036).** Report-only
LocalId→source-span metadata survives HIR lowering and the local-cloning
transforms, fills the report's existing `byte_offset` field for named locals
(JSON schema unchanged), and text reports render file:line:column with a
source/caret snippet — including CJS wrapper line correction — so a denial
points at the declaration instead of leaving the reader to grep for it.
(Fragment added at merge.)
