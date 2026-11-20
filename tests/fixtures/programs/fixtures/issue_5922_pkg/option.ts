// Mirrors effect's `Option.ts` — same shape as context.ts, deliberately
// exporting a member with the identical bare name "a" so a single
// importing file that pulls in both namespaces exercises the collision.
export function a(): string {
  return "option-a"
}
export function make(): string {
  return "option-make"
}
