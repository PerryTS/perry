import { AsyncResource, createHook } from "node:async_hooks";

function construct(label: string) {
  try {
    const resource = new AsyncResource("");
    console.log(label, "empty type: ok");
    resource.emitDestroy();
  } catch (error) {
    const typed = error as { code?: string; name: string };
    console.log(label, "empty type:", typed.name, typed.code || "no-code");
  }
}

construct("hooks disabled");

const hook = createHook({ init() {} }).enable();
construct("hooks enabled");
hook.disable();

construct("hooks disabled again");
