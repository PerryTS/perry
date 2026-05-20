import { setTimeout } from "node:timers";
// `let` in a for-loop creates a fresh binding per iteration — the
// timer callback must capture the iteration's `i`, not the final value.
const seen: number[] = [];
await new Promise<void>((resolve) => {
  for (let i = 0; i < 3; i++) {
    setTimeout(() => {
      seen.push(i);
      if (seen.length === 3) resolve();
    }, 1);
  }
});
seen.sort();
console.log("captured:", seen.join(","));
