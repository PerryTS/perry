// Issue #2013 — Node-shaped argument validation for the process
// surface (`chdir`/`kill`/`exit`/`cpuUsage`/`hrtime`). Each probe
// prints the thrown error's `.code` and `.name`; Perry and Node must
// produce the exact same lines. `process.exit` is intentionally not
// probed (the success path tears the whole process down) and the
// no-error case is exercised by the parent suite's `cwd-basic.ts`.

function probe(label: string, fn: () => any) {
  try {
    fn();
    console.log(label, "no-throw");
  } catch (e: any) {
    console.log(label, e.name, e.code);
  }
}

// process.chdir(directory)
probe("chdir(123)", () => process.chdir(123 as any));
probe("chdir({})", () => process.chdir({} as any));
probe("chdir(null)", () => process.chdir(null as any));
probe("chdir(true)", () => process.chdir(true as any));

// process.kill(pid, signal?)
probe("kill('abc',0)", () => process.kill("abc" as any, 0));
probe("kill({},0)", () => process.kill({} as any, 0));
probe("kill(0,{})", () => process.kill(0, {} as any));
probe("kill(0,true)", () => process.kill(0, true as any));

// process.cpuUsage(prior?) — undefined / null pass through, non-object throws.
probe("cpuUsage('abc')", () => process.cpuUsage("abc" as any));
probe("cpuUsage(123)", () => process.cpuUsage(123 as any));
probe("cpuUsage(true)", () => process.cpuUsage(true as any));

// process.hrtime(prior?) — undefined passes through, non-array throws.
probe("hrtime('abc')", () => process.hrtime("abc" as any));
probe("hrtime(123)", () => process.hrtime(123 as any));
probe("hrtime({})", () => process.hrtime({} as any));
