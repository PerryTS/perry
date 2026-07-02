// Type declarations for the bare `perry` module — Perry's standalone-executable
// embedded-asset API (#5731). These let `tsc` / IDEs resolve
// `import { embeddedFiles, readEmbedded, isStandaloneExecutable } from "perry"`.
//
// Assets are baked into the binary at compile time via
// `perry compile --embed "./dist/**"` (or `perry.embed` in package.json /
// `[compile] embed` in perry.toml). At runtime they are also readable through
// `node:fs` via their `$perryfs/<path>` virtual path.

/** Metadata for a single asset embedded into the standalone executable. */
export interface EmbeddedFile {
  /** Embed-relative path, e.g. `dist/index.html`. Use as the route key. */
  readonly name: string;
  /** Size of the asset in bytes. */
  readonly size: number;
  /** Best-effort MIME type inferred from the file extension. */
  readonly type: string;
}

/**
 * All files embedded into this executable, sorted by their embed-relative path
 * (deterministic across builds, after de-duplication). Returns a fresh array on
 * each call (empty for a non-embedded build).
 *
 * Note: exposed as a function (not a bare value like Bun's `embeddedFiles`) so
 * that array methods dispatch correctly on the result —
 * `embeddedFiles().map(f => f.name)`.
 */
export function embeddedFiles(): ReadonlyArray<EmbeddedFile>;

/**
 * `true` when running as a Perry-compiled standalone executable. Always `true`
 * at runtime (Perry has no interpreter mode); useful as a dev-vs-compiled guard
 * in code shared with a Node/tsx dev workflow.
 */
export const isStandaloneExecutable: boolean;

/**
 * Read an embedded asset's bytes. Accepts either the `$perryfs/<path>` virtual
 * path or the embed-relative key (`dist/index.html`). Returns the bytes as a
 * `Buffer` (a `Uint8Array`); throws an `Error` when the asset is not found.
 */
export function readEmbedded(path: string): Buffer;
