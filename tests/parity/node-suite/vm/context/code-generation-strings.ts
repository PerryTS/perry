import * as vm from "node:vm";

const disabled = vm.createContext({}, {
  codeGeneration: { strings: false, wasm: true },
});
try {
  console.log("eval:", vm.runInContext("eval('1')", disabled));
} catch (error: any) {
  console.log("eval:", error.name, error.code || "-");
}
try {
  console.log(
    "Function:",
    vm.runInContext("new Function('return 1')()", disabled),
  );
} catch (error: any) {
  console.log("Function:", error.name, error.code || "-");
}

const enabled = vm.createContext({}, { codeGeneration: { strings: true } });
console.log("enabled eval:", vm.runInContext("eval('2 + 3')", enabled));
