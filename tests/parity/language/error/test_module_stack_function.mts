const functions = [function moduleCalled(n: number) {
  const stack = new Error("called " + n).stack || "";
  console.log("function frame", stack.includes("at moduleCalled ("));
  console.log("module frame", /^\s*at file:\/\/.*\/test_module_stack_function\.mts(?::\d+:\d+)?$/m.test(stack));
  return n;
}];
const fn = functions[Number(process.argv[2] || 0)];
console.log("result", fn(1));
