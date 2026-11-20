// Issue #6304: the namespace object of a dynamically-imported PURE RE-EXPORT
// chunk must expose the re-exported bindings.
//
// `dynamic_import_chunk_agent.ts` is the two-statement re-export file esbuild
// and bun emit for a shared chunk under `--splitting` (`import { run } from
// "./chunk-XXXX.js"; export { run };`). Pre-fix its namespace came out empty:
// `typeof ns.run` was `undefined`, and an unguarded `ns.run()` printed
// `undefined` rather than throwing — a SILENT wrong answer that quietly broke
// every code-split bundle.
//
// The worker chunk is the control: a chunk that contains real code (not just
// re-exports) already worked and must keep working.

async function main(): Promise<void> {
  const ns = await import("./dynamic_import_chunk_agent.ts");
  console.log("typeof ns:", typeof ns);
  console.log("typeof ns.run:", typeof ns.run);
  console.log("ns.run():", ns.run("agent"));
  // Re-export under a renamed key resolves to the shared chunk's binding.
  console.log("ns.version:", ns.version);

  const w = await import("./dynamic_import_chunk_worker.ts");
  console.log("typeof w.work:", typeof w.work);
  console.log("w.work():", w.work());
}

main();
