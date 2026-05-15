// Re-exporter barrel for issue #678. The `default as renamed` shape is
// the canonical npm-barrel pattern (ink does `export { default as Box }
// from './components/Box.js'`, etc.). Pre-fix the consumer's codegen
// formed `perry_fn_<inner>__renamedArrow` and `perry_fn_<inner>__renamedPlain`
// even though the origin module emits the symbol under `default` /
// `plain`.

export { default as renamedArrow } from "./sub/inner.ts";
export { plain as renamedPlain } from "./sub/inner.ts";
