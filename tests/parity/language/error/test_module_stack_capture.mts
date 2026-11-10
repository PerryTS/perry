const target: { stack?: string } = {};
Error.captureStackTrace(target);
const stack = target.stack || "";
console.log("capture module", /^\s*at file:\/\/.*\/test_module_stack_capture\.mts(?::\d+:\d+)?$/m.test(stack));
