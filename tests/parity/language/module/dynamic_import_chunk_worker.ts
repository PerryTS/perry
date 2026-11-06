// Helper for test_gap_dynamic_import_reexport_chunk.ts — the CONTROL chunk.
//
// Unlike the agent chunk, this one contains real code (a local function) on top
// of its imports from the shared chunk. This shape already worked before #6304;
// keeping it in the fixture guards against a fix that trades the re-export-only
// case for the ordinary one.
import { run, VERSION } from "./dynamic_import_chunk_shared.ts";

export function work(): string {
  return run("worker") + " v" + VERSION;
}
