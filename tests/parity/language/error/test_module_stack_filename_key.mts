const frame = (new Error().stack || "").split("\n")[1] || "";
const filename = frame.replace(/^\s*at /, "").replace(/:\d+:\d+$/, "");
const registry: Record<string, string> = {};
registry[filename] = "registered";
console.log("filename key", filename.startsWith("file:///") && filename.endsWith("/test_module_stack_filename_key.mts"));
console.log("lookup", registry[filename]);
