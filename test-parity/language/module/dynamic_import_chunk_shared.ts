// Helper for test_gap_dynamic_import_reexport_chunk.ts — stands in for the
// `chunk-XXXX.js` esbuild/bun hoist the shared code into under `--splitting`.
// This module holds the REAL definitions; the sibling agent/worker chunks
// only forward to it.
export function run(tag: string): string {
  return "[shared-util] " + tag;
}

export const VERSION = "1.0.0";
