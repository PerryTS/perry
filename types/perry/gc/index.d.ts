// Type declarations for perry/gc — explicit garbage-collection control.
// These types are auto-written by `perry init` / `perry types` so IDEs
// and tsc can resolve `import { ... } from "perry/gc"`.
//
// This module is a Perry-native pacing surface (like `perry/thread`): it
// does not resolve under Node/Bun. Guard imports if the same source must
// also run there.

/**
 * Run a full garbage collection now. Same semantics as the global `gc()`:
 * a complete mark-sweep that also reclaims dead large/tenured objects and
 * returns freed memory to the OS.
 */
export function collect(): void;

/**
 * Run a minor (nursery-only) collection now and return the number of freed
 * bytes. Cheaper than `collect()`; useful for explicit pacing in
 * latency-sensitive loops.
 */
export function minor(): number;

/**
 * Declare an idle point (e.g. a frame boundary). If a threshold-driven
 * collection is already due, it runs here — at a moment you chose — instead
 * of landing mid-frame at an arbitrary allocation site. O(1) when nothing
 * is due. Returns whether a collection ran.
 */
export function idleHint(): boolean;
