// Pushes into a local array that nothing else holds take a shorter
// admission; every way of handing the array out keeps the full one.

function owned(n: number): number {
  const xs: number[] = [];
  for (let i = 0; i < n; i++) xs.push(i * 2);
  return xs.length + (xs[n - 1] ?? 0);
}
console.log("owned", owned(5), owned(40), owned(300));

function ownedReturned(n: number): number[] {
  const xs: number[] = [];
  for (let i = 0; i < n; i++) xs.push(i);
  return xs;
}
const r = ownedReturned(33);
console.log("returned", r.length, r[0], r[32], Object.isFrozen(r));

// Mixed values: strings and objects go through the bookkeeping arm.
function ownedMixed(n: number): string {
  const xs: any[] = [];
  for (let i = 0; i < n; i++) xs.push(i % 3 === 0 ? "s" + i : i % 3 === 1 ? { v: i } : i);
  return xs.length + ":" + xs[0] + ":" + JSON.stringify(xs[1]) + ":" + xs[2];
}
console.log("mixed", ownedMixed(50));

// Frozen through an alias: push must throw.
function frozenAlias(): string {
  const xs: number[] = [];
  xs.push(1);
  const alias = xs;
  Object.freeze(alias);
  try {
    xs.push(2);
    return "no throw";
  } catch (e) {
    return (e as Error).constructor.name + " " + xs.length;
  }
}
console.log("frozen", frozenAlias());

// An own `push` installed through an alias.
function ownPush(): string {
  const xs: number[] = [];
  const holder = { xs };
  (holder.xs as any).push = (v: number) => "own:" + v;
  return String(xs.push(7)) + " " + xs.length;
}
console.log("own push", ownPush());

// Grown through an alias while the loop pushes.
function aliasGrown(): string {
  const xs: number[] = [];
  const other = [xs];
  for (let i = 0; i < 30; i++) {
    xs.push(i);
    if (i % 7 === 0) other[0].push(-i);
  }
  return xs.length + " " + xs[0] + " " + xs[1] + " " + xs[xs.length - 1];
}
console.log("alias grown", aliasGrown());

// A closure captures the array.
function captured(): string {
  const xs: number[] = [];
  const freeze = () => Object.freeze(xs);
  xs.push(1);
  freeze();
  try { xs.push(2); return "no throw"; } catch (e) { return (e as Error).constructor.name; }
}
console.log("captured", captured());

// Allocation pressure while pushing.
function pressure(): number {
  let total = 0;
  for (let round = 0; round < 200; round++) {
    const xs: number[] = [];
    for (let i = 0; i < 64; i++) {
      xs.push(i + round);
      const junk = { a: i, b: "x" + i };
      total += junk.a & 1;
    }
    total += xs.length + (xs[63] ?? 0);
  }
  return total;
}
console.log("pressure", pressure());
