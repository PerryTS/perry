const callback = (n: number) => {
  if (n > 0) callback(n - 1);
  else {
    const stack = new Error("arrow").stack || "";
    console.log("callback frame", stack.includes("at callback ("));
    console.log("module frame", /^\s*at file:\/\/.*\/test_module_stack_arrow\.mts(?::\d+:\d+)?$/m.test(stack));
  }
};
[Number(process.argv[2] || 0)].forEach(callback);
