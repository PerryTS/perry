import { AsyncLocalStorage, AsyncResource } from "node:async_hooks";

function probe(label: string, operation: () => unknown) {
  try {
    operation();
    console.log(label, "no-throw");
  } catch (error: any) {
    console.log(label, error.name, error.code || "no-code");
  }
}

probe("AsyncResource call", () => (AsyncResource as any)("CallValidation"));
probe("AsyncLocalStorage call", () => (AsyncLocalStorage as any)());

const resource = new AsyncResource("ConstructValidation");
const storage = new AsyncLocalStorage<string>();
console.log(
  "constructor instances:",
  resource instanceof AsyncResource,
  storage instanceof AsyncLocalStorage,
);
resource.emitDestroy();
storage.disable();
