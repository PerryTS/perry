// #8432: strings owned by module roots, closure/box cells, and async
// activation boxes must reach the amortized append path. Ordinary reads still
// extract aliases and must demote the owner before later in-place growth.

const PART = "[abc]";

let moduleAccumulator = "";

function buildModuleGlobal(n: number): string {
  moduleAccumulator = "";
  let snapshot = "";
  for (let i = 0; i < n; i++) {
    moduleAccumulator = moduleAccumulator + "[" + "abc" + "]";
    if (i === (n >> 1)) snapshot = moduleAccumulator;
  }
  return [
    moduleAccumulator.length,
    snapshot.length,
    snapshot.slice(-5),
    moduleAccumulator.slice(-5),
  ].join(":");
}

function buildCaptured(n: number): string {
  let accumulator = "";
  let snapshot = "";
  const append = () => {
    accumulator = accumulator + PART;
  };

  for (let i = 0; i < n; i++) {
    append();
    if (i === (n >> 1)) snapshot = accumulator;
  }
  return [accumulator.length, snapshot.length, snapshot === accumulator].join(":");
}

async function buildAsync(n: number): Promise<string> {
  let accumulator = "";
  let snapshot = "";
  for (let i = 0; i < n; i++) {
    accumulator = accumulator + "[" + "abc" + "]";
    if (i === (n >> 1)) snapshot = accumulator;
  }

  // Touch the completed bytes at a nontrivial stride so the fixture observes
  // materialized string contents, not only metadata.
  let checksum = 0;
  for (let i = 0; i < accumulator.length; i += 997) {
    checksum += accumulator.charCodeAt(i);
  }
  return [
    accumulator.length,
    snapshot.length,
    snapshot === accumulator,
    checksum,
    accumulator.slice(0, 5),
    accumulator.slice(-5),
  ].join(":");
}

console.log("global", buildModuleGlobal(2_000));
console.log("capture", buildCaptured(2_000));
buildAsync(2_000).then((result) => console.log("async", result));
