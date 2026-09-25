// Two stars export `dup` with different origins (ambiguous, so it is
// dropped). The names only one of them exports must still resolve.
import { one } from "./amb1.ts";
export { one as oneAgain };
export * from "./amb1.ts";
export * from "./amb2.ts";
