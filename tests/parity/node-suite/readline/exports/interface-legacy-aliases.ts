import * as readline from "node:readline";

const prototype = (readline as any).Interface?.prototype;
if (prototype) {
  const names = [
    "_setRawMode",
    "_onLine",
    "_writeToOutput",
    "_addHistory",
    "_refreshLine",
    "_normalWrite",
    "_insertString",
    "_tabComplete",
    "_wordLeft",
    "_wordRight",
    "_deleteLeft",
    "_deleteRight",
    "_line",
    "_getCursorPos",
    "_moveCursor",
    "_ttyWrite",
  ];
  console.log(
    names.map((name) => `${name}:${typeof prototype[name]}`).join("|"),
  );
} else {
  console.log("missing");
}
