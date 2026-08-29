// A static z.object schema should take Perry's schema-aware native path even
// when the runtime was built without dyn-eval. Invalid input still delegates
// to Zod so its normal issue construction remains authoritative.
import * as z from "zod";

// Keep Zod's older object-parser JIT out of this test. The feature under test
// is the explicit z.compile() call below, which Perry replaces before runtime.
z.config({ jitless: true });

const Player = z.object({
  name: z.string(),
  score: z.number().int().min(0).max(100),
  active: z.boolean(),
});

const originalRun = Player._zod.run;
let fallbackRuns = 0;
Player._zod.run = (payload, ctx) => {
  fallbackRuns++;
  return originalRun(payload, ctx);
};

const CompiledPlayer = z.compile(Player, { strict: true });
const valid = CompiledPlayer.safeParse({
  name: "Billie",
  score: 42,
  active: true,
  unknown: "strip me",
});
const fallbackRunsAfterValid = fallbackRuns;
const invalid = CompiledPlayer.safeParse({
  name: "Billie",
  score: "not a number",
  active: true,
});

console.log(CompiledPlayer !== Player);
console.log(CompiledPlayer._zod.run !== Player._zod.run);
console.log(
  (CompiledPlayer._zod.run as typeof Player._zod.run & {
    __originalRun?: typeof Player._zod.run;
  }).__originalRun === Player._zod.run,
);
console.log(CompiledPlayer._zod.bag.fallbackRun === Player._zod.run);
console.log(valid.success);
console.log(fallbackRunsAfterValid === 0);
console.log(valid.success && !("unknown" in valid.data));
console.log(invalid.success);
console.log(fallbackRuns === 1);
if (!invalid.success) {
  console.log(`${invalid.error.issues[0].path.join(".")}:${invalid.error.issues[0].code}`);
}
