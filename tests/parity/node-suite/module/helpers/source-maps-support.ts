import * as Module from "node:module";

function formatState(state) {
  return JSON.stringify({
    enabled: state.enabled,
    nodeModules: state.nodeModules,
    generatedCode: state.generatedCode,
  });
}

function errorLine(label, fn) {
  try {
    fn();
    console.log(`${label}: no throw`);
  } catch (error) {
    console.log(`${label}:`, error.name, error.code, error.message);
  }
}

console.log("before:", formatState(Module.getSourceMapsSupport()));
console.log("set true:", String(Module.setSourceMapsSupport(true)));
console.log("after true:", formatState(Module.getSourceMapsSupport()));
console.log(
  "set options:",
  String(
    Module.setSourceMapsSupport(true, {
      nodeModules: true,
      generatedCode: true,
    }),
  ),
);
console.log("after options:", formatState(Module.getSourceMapsSupport()));
console.log("set false:", String(Module.setSourceMapsSupport(false)));
console.log("after false:", formatState(Module.getSourceMapsSupport()));

const capturedSetSourceMapsSupport = Module.setSourceMapsSupport;
const capturedGetSourceMapsSupport = Module.getSourceMapsSupport;
console.log("captured set true:", String(capturedSetSourceMapsSupport(true)));
console.log("captured get:", formatState(capturedGetSourceMapsSupport()));
capturedSetSourceMapsSupport(false);

errorLine("bad enabled", () => Module.setSourceMapsSupport({}));
errorLine("bad nodeModules", () =>
  Module.setSourceMapsSupport(true, { nodeModules: "yes" }),
);
