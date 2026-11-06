// Mirrors effect's `Context.ts` — a namespace re-exported from the
// package's main barrel, with a member name ("a") that also happens to be
// exported (with different behavior) by a sibling namespace module
// (option.ts) re-exported through the same barrel.
export function a(): string {
  return "context-a"
}
export function make(): string {
  return "context-make"
}
