import { EventEmitterAsyncResource } from "node:events";

function show(label: string, create: () => EventEmitterAsyncResource) {
  try {
    const resource = create();
    console.log(
      label,
      "ok",
      typeof (resource as any).asyncId,
      typeof (resource as any).triggerAsyncId,
    );
  } catch (error: any) {
    console.log(
      label,
      "error",
      error.name,
      error.code ?? "",
      String(error.message).includes("options.name"),
    );
  }
}

show("undefined", () => new (EventEmitterAsyncResource as any)());
show("empty object", () => new EventEmitterAsyncResource({} as any));
show("null", () => new EventEmitterAsyncResource(null as any));
show("string", () => new EventEmitterAsyncResource("perry-resource" as any));
show("object", () => new EventEmitterAsyncResource({ name: "perry-resource" }));
