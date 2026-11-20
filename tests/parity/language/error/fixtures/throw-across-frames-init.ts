// Fixture of test_throw_across_frames.ts: module init throws from a frame below
// the module body.
function initHelper(): never {
  throw new Error("module init failed");
}
export const value: number = initHelper();
