import { addAbortListener } from "node:events";

function show(label: string, fn: () => any): void {
  try {
    const value = fn();
    console.log(label, "OK", typeof value?.[Symbol.dispose]);
  } catch (error) {
    const e = error as Error & { code?: string };
    console.log(label, "THROW", e.name, e.code, String(e.message).split("\n")[0]);
  }
}

const signal = new AbortController().signal;

show("missing signal", () => addAbortListener());
show("number signal", () => addAbortListener(123 as any, () => {}));
show("object signal", () => addAbortListener({} as any, () => {}));
show("missing listener", () => addAbortListener(signal));
show("number listener", () => addAbortListener(signal, 123 as any));
show("object listener", () => addAbortListener(signal, {} as any));
show("valid", () => addAbortListener(signal, () => {}));
