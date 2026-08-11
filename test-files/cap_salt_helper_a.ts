// Support module for test_gap_cap_salt_reproducible_7177.ts (#7177).
// Deliberately mirrors cap_salt_helper_b.ts: same shape, same local names, so
// the only thing distinguishing their capture stashes is the module salt.
export function makeA(seed: number): number {
  const captured = seed + 1;
  class Holder {
    get(): number {
      return captured;
    }
  }
  return new Holder().get();
}
