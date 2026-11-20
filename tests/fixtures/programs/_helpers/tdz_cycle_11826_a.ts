import { callPeek, readA } from "./tdz_cycle_11826_b.ts";

export function peekPrivate(): number {
  return PRIVATE;
}

try {
  console.log(readA());
} catch (error: any) {
  console.log(error.constructor.name);
}
try {
  console.log(callPeek());
} catch (error: any) {
  console.log(error.constructor.name);
}

export const A = 7;
const PRIVATE = 11;
console.log(readA());
console.log(callPeek());
