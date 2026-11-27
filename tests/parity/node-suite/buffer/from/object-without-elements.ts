// Buffer.from(object) takes an object's string value only from a callable
// Symbol.toPrimitive, valueOf or an array-like shape; any other object is an
// ERR_INVALID_ARG_TYPE TypeError, never an empty Buffer.

function show(label: string, run: () => unknown): void {
  try {
    console.log(label, String(run()));
  } catch (e) {
    const err = e as Error & { code?: string };
    console.log(label, err.constructor.name, err.code);
  }
}

show("plain object", () => Buffer.from({} as any).length);
show("non-callable toPrimitive", () => Buffer.from({ [Symbol.toPrimitive]: 42 } as any).length);
show("toPrimitive object result", () => Buffer.from({ [Symbol.toPrimitive]: () => [1] } as any).length);
show("Map", () => Buffer.from(new Map() as any).length);
show("toPrimitive string", () => Buffer.from({ [Symbol.toPrimitive]: () => "ab" } as any).toString());
show("valueOf string", () => Buffer.from({ valueOf: () => "cd" } as any).toString());
show("array", () => Buffer.from([1, 2, 3]).length);
show("array-like", () => Buffer.from({ length: 2, 0: 7, 1: 8 } as any).join());
show("typed array", () => Buffer.from(new Uint8Array([4, 5])).join());
