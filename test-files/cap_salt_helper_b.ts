// Support module for test_gap_cap_salt_reproducible_7177.ts (#7177).
// See cap_salt_helper_a.ts — identical shape, different module identity.
export function makeB(seed: number): number {
  const captured = seed + 2;
  class Holder {
    get(): number {
      return captured;
    }
  }
  return new Holder().get();
}
