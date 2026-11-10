function capturedFromFunction(n: number): { stack?: string } {
  const target: { stack?: string } = {};
  Error.captureStackTrace(target);
  return target;
}
const functions = [capturedFromFunction];
const capture = functions[Number(process.argv[2] || 0)];
const target = capture(1);
// Read after the function has returned: these must be the capture-site frames.
const stack = target.stack || "";
const frames = stack.split("\n").slice(1);
const own = frames.filter(line => line.includes("at capturedFromFunction (") || /^\s*at file:\/\/.*\/test_module_stack_capture_function\.mts(?::\d+:\d+)?$/.test(line));
console.log("source frames", own.length);
console.log("function first", own[0] && own[0].includes("at capturedFromFunction ("));
console.log("module second", own[1] && /^\s*at file:\/\//.test(own[1]));
