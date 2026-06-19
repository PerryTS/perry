// Test: `||` / `&&` / `??` short-circuit when the RHS operand contains an
// `await` (#5434). The async→generator pre-pass hoisted any non-top-level
// await into a `let __awaitN = await ...` placed *before* the containing
// statement, which evaluated the guarded RHS eagerly — so a guard clause like
// `if (!user || !(await verify(user.passwordHash)))` read `user.passwordHash`
// even when `!user` was already true, throwing on null. The fix lifts a
// logical whose RHS contains an await into a guarded if/else with a temp, so
// the RHS await only fires when the LHS does not short-circuit.
// Validated byte-for-byte against `node --experimental-strip-types`.

let fired = 0;
async function val(x: number): Promise<number> { fired++; return x; }
async function tf(b: boolean): Promise<boolean> { fired++; return b; }

async function main(): Promise<void> {
  // --- the original repro: guard clause never reads the guarded prop ---
  const user: { passwordHash: string } | null = null;
  let threw = false;
  try {
    const bad = (!user || !(await tf((user as any).passwordHash.length > 0)));
    console.log("guard ||:", bad); // true
  } catch { threw = true; }
  console.log("guard threw:", threw); // false

  // --- || : LHS truthy short-circuits, RHS not fired ---
  fired = 0;
  console.log("A", (5 || (await val(9))), "fired", fired); // A 5 fired 0
  // --- || : LHS falsy fires RHS ---
  fired = 0;
  console.log("B", (0 || (await val(9))), "fired", fired); // B 9 fired 1
  // --- && : LHS falsy short-circuits, RHS not fired ---
  fired = 0;
  console.log("C", (0 && (await val(9))), "fired", fired); // C 0 fired 0
  // --- && : LHS truthy fires RHS ---
  fired = 0;
  console.log("D", (5 && (await val(9))), "fired", fired); // D 9 fired 1
  // --- ?? : LHS non-null short-circuits (falsy-but-defined stays) ---
  fired = 0;
  console.log("E", (0 ?? (await val(9))), "fired", fired); // E 0 fired 0
  // --- ?? : null and undefined both fire RHS ---
  fired = 0;
  console.log("F", (null ?? (await val(9))), "fired", fired); // F 9 fired 1
  fired = 0;
  console.log("G", (undefined ?? (await val(7))), "fired", fired); // G 7 fired 1
  // --- await in LHS is always evaluated; RHS still short-circuits ---
  fired = 0;
  console.log("H", ((await tf(true)) || (await val(9))), "fired", fired); // H true fired 1
  // --- chained logicals ---
  fired = 0;
  console.log("I", (0 || "" || (await val(3))), "fired", fired); // I 3 fired 1
  // --- nested logical inside the guarded RHS ---
  fired = 0;
  console.log("J", (false || ((await tf(true)) && (await val(2)))), "fired", fired); // J 2 fired 2
  // --- return position ---
  console.log("K", await retGuard(null)); // K true
}

async function retGuard(u: { p: string } | null): Promise<boolean> {
  return !u || (await tf((u as any).p.length > 0));
}

void main();
