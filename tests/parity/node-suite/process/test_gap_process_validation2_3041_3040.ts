// Gap test: process argument validation (#3041 exit, #3043 chdir,
// #3049 setMaxListeners, #3039 hrtime, #3040 cpuUsage).
//
// process.exit validation/coercion is tested WITHOUT exiting: only the
// throwing/coercion paths are exercised (we never call exit with a value
// that would actually terminate).

function show(label: string, fn: () => void): void {
  try {
    fn();
    console.log(label, "OK");
  } catch (e: any) {
    console.log(label, "THROW", e.name, e.code, e.message.split("\n")[0]);
  }
}

// --- #3041 process.exit code validation (throw paths only) ---
const exitCases: any[] = ["2.5", "abc", true, 1.9, NaN, Infinity, {}, [], "", "1_0", "NaN"];
for (const c of exitCases) {
  show("exit " + JSON.stringify(c), () => {
    process.exit(c);
  });
}

// --- #3043 process.chdir validation (dynamic / method-value) ---
const chdir = process.chdir;
show("chdir(123)", () => {
  process.chdir(123 as any);
});
show("bound chdir(123)", () => {
  chdir(123 as any);
});
show("chdir({})", () => {
  process.chdir({} as any);
});

// --- #3049 process.setMaxListeners ---
const maxCases: any[] = [0, 1, 1.5, Infinity, -1, NaN, "5", null, undefined];
for (const value of maxCases) {
  show("max " + String(value), () => {
    process.setMaxListeners(value);
    console.log("  -> read", process.getMaxListeners());
  });
}

// --- #3039 process.hrtime prior tuple validation ---
const hrCases: any[] = [[], [0], [0, 0], [0, 0, 0], null, "x", 1, {}];
for (const prior of hrCases) {
  show("hrtime " + JSON.stringify(prior), () => {
    process.hrtime(prior);
  });
}

// --- #3040 process.cpuUsage previous value validation ---
const cpuCases: any[] = [
  {},
  [],
  { user: 0, system: 0 },
  { user: "1", system: "2" },
  { user: -1, system: 0 },
  { user: Infinity, system: 0 },
  { user: 1 },
  { system: 1 },
  "x",
  1,
];
for (const prior of cpuCases) {
  show("cpu " + JSON.stringify(prior), () => {
    process.cpuUsage(prior);
  });
}
