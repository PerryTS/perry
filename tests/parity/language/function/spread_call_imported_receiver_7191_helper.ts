// Support module for test_gap_spread_call_imported_receiver_7191.ts (#7191).
// The receivers must be IMPORTED — that is the whole subject of the test.
export const arr = [3, 1, 2];
export const nums: number[] = [10, 20, 30];
export function fn(a: number, b: number) {
  return a + b;
}
export const obj = {
  m(x: number) {
    return x * 3;
  },
  v: 7,
};
export class K {
  static s(x: number) {
    return x + 100;
  }
  m(x: number) {
    return x - 1;
  }
}
