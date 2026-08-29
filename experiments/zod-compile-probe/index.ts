import * as z from "zod";

// Isolate explicit z.compile() from Zod's older object-parser JIT.
z.config({ jitless: true });

const Player = z.object({
  username: z.string(),
  displayName: z.string(),
  email: z.string(),
  country: z.string(),
  language: z.string(),
  team: z.string(),
  role: z.string(),
  status: z.string(),
  bio: z.string(),
  avatar: z.string(),
  xp: z.number().int().min(0).max(1000),
  level: z.number(),
  rank: z.number(),
  wins: z.number(),
  losses: z.number(),
  streak: z.number(),
  coins: z.number(),
  gems: z.number(),
  active: z.boolean(),
  verified: z.boolean(),
});

const originalRun = Player._zod.run;
let fallbackRuns = 0;
Player._zod.run = (payload, ctx) => {
  fallbackRuns++;
  return originalRun(payload, ctx);
};

// strict:true makes an unsupported compiler path throw instead of returning
// the original interpreted schema.
const CompiledPlayer = z.compile(Player, { strict: true });

const validPlayer = {
  username: "billie",
  displayName: "Billie",
  email: "billie@example.com",
  country: "DE",
  language: "de",
  team: "blue",
  role: "captain",
  status: "online",
  bio: "validator",
  avatar: "avatar.png",
  xp: 100,
  level: 4,
  rank: 12,
  wins: 8,
  losses: 2,
  streak: 3,
  coins: 500,
  gems: 25,
  active: true,
  verified: true,
  strippedByObjectSchema: "yes",
};

const invalidPlayer = {
  username: "billie",
  displayName: "Billie",
  email: "billie@example.com",
  country: "DE",
  language: "de",
  team: "blue",
  role: "captain",
  status: "online",
  bio: "validator",
  avatar: "avatar.png",
  xp: "not-a-number",
  level: 4,
  rank: 12,
  wins: 8,
  losses: 2,
  streak: 3,
  coins: 500,
  gems: 25,
  active: true,
  verified: true,
};

const valid = CompiledPlayer.safeParse(validPlayer);
const fallbackRunsAfterValid = fallbackRuns;
const invalid = CompiledPlayer.safeParse(invalidPlayer);

console.log(`compiledClone=${CompiledPlayer !== Player}`);
console.log(`compiledRunChanged=${CompiledPlayer._zod.run !== Player._zod.run}`);
console.log(
  `wrapperOwnedByZod=${
    (CompiledPlayer._zod.run as typeof Player._zod.run & {
      __originalRun?: typeof Player._zod.run;
    }).__originalRun === Player._zod.run
  }`,
);
console.log(
  `fallbackOwnedByZod=${CompiledPlayer._zod.bag.fallbackRun === Player._zod.run}`,
);
console.log(`validSuccess=${valid.success}`);
console.log(`validAvoidedFallback=${fallbackRunsAfterValid === 0}`);
console.log(
  `validStrippedUnknown=${
    valid.success && !("strippedByObjectSchema" in valid.data)
  }`,
);
console.log(`invalidSuccess=${invalid.success}`);
console.log(`invalidUsedFallback=${fallbackRuns === 1}`);
if (!invalid.success) {
  const issue = invalid.error.issues[0];
  console.log(`invalidIssue=${issue.path.join(".")}:${issue.code}`);
}
