import { A, peekPrivate } from "./tdz_cycle_11826_a.ts";

export function readA(): number {
  return A;
}

export function callPeek(): number {
  return peekPrivate();
}
