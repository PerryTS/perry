const stack = new Error("scope").stack || "";
const lines = stack.split("\n").slice(1);
console.log("module frames", lines.filter(line => /^\s*at file:\/\/.*\/test_module_stack_scope\.mts(?::\d+:\d+)?$/.test(line)).length);
console.log("no runtime pcs", !/rt\+0x|js_error_new/.test(stack));
