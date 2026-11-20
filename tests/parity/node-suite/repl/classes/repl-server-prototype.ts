import { REPLServer } from "node:repl";

for (
  const name of [
    "setupHistory",
    "clearBufferedCommand",
    "createContext",
    "resetContext",
    "displayPrompt",
    "setPrompt",
    "complete",
    "completeOnEditorMode",
    "defineCommand",
  ]
) {
  const descriptor = Object.getOwnPropertyDescriptor(
    REPLServer.prototype,
    name,
  );
  console.log(
    name,
    typeof descriptor?.value,
    descriptor?.enumerable,
    descriptor?.configurable,
    descriptor?.writable,
  );
}
