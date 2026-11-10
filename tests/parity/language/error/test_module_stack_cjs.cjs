const stack = (new Error("commonjs").stack || "").replace(/\\/g, "/");
const first = stack.split("\n")[1] || "";
console.log("CJS module", /^\s*at Object\.<anonymous> \(.+\/test_module_stack_cjs\.cjs(?::\d+:\d+)?\)$/.test(first));
console.log("source frames", stack.split("\n").filter(line => line.includes("/test_module_stack_cjs.cjs")).length);
