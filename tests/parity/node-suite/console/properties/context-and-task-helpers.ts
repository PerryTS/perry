import * as consoleModule from "node:console";

function errorShape(fn: () => unknown): string {
  try {
    fn();
    return "no error";
  } catch (error) {
    const err = error as Error & { code?: string };
    return `${err.name}:${err.code ?? "no-code"}:${err.message}`;
  }
}

console.log(
  "module keys:",
  Object.keys(consoleModule).includes("context"),
  Object.keys(consoleModule).includes("createTask"),
);

for (const name of ["context", "createTask"] as const) {
  const value = consoleModule[name] as unknown as Function;
  const desc = Object.getOwnPropertyDescriptor(consoleModule, name);
  if (name === "context") {
    console.log(`${name} type/name:`, typeof value, value.name);
  } else {
    console.log(`${name} type/name/length:`, typeof value, value.name, value.length);
  }
  console.log(`${name} descriptor:`, desc?.enumerable, desc?.writable);
}

const scoped = consoleModule.context("scope");
const scopedAgain = consoleModule.context("scope");
console.log("context distinct:", scoped !== scopedAgain);
console.log("context keys:", Object.keys(scoped).sort().join(","));
console.log(
  "context log shape:",
  typeof scoped.log,
  scoped.log.name,
  scoped.log.length,
);
console.log(
  "context helpers:",
  ["assert", "clear", "dirXml", "groupEnd", "timeStamp", "warn"]
    .map((name) => `${name}:${typeof (scoped as any)[name]}:${(scoped as any)[name].length}`)
    .join(","),
);
console.log("context noargs:", typeof consoleModule.context());
console.log(
  "context symbol error:",
  errorShape(() => consoleModule.context(Symbol("scope") as any)),
);

const task = consoleModule.createTask("task");
console.log("task keys:", Object.keys(task).sort().join(","));
console.log("task run shape:", typeof task.run, task.run.name, task.run.length);
console.log("task run return:", task.run(() => 42));
let sideEffect = 0;
task.run(() => {
  sideEffect = 7;
});
console.log("task side effect:", sideEffect);
console.log("task invalid name:", errorShape(() => consoleModule.createTask("")));
console.log("task non-string name:", errorShape(() => consoleModule.createTask(123 as any)));
console.log("task run invalid:", errorShape(() => task.run(123 as any)));
console.log(
  "task run thrown:",
  errorShape(() =>
    task.run(() => {
      throw new Error("boom");
    }),
  ),
);
