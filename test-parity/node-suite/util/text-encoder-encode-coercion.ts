import { TextEncoder } from "node:util";

const encoder = new TextEncoder();

function show(label: string, fn: () => Uint8Array): void {
  try {
    const bytes = fn();
    console.log(`${label}:`, Array.from(bytes).join(","));
  } catch (error: any) {
    console.log(
      `${label}:`,
      error?.name,
      error?.code === undefined,
      String(error?.message).split("\n")[0],
    );
  }
}

show("encode omitted", () => encoder.encode());
show("encode undefined", () => encoder.encode(undefined));
show("encode null", () => encoder.encode(null as any));
show("encode number", () => encoder.encode(123 as any));
show("encode boolean", () => encoder.encode(false as any));
show("encode object", () =>
  encoder.encode({
    toString() {
      return "obj";
    },
  } as any),
);
show("encode array", () => encoder.encode(["a", 2] as any));
show("encode symbol", () => encoder.encode(Symbol("x") as any));
