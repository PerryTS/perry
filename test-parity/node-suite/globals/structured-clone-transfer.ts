function show(label: string, fn: () => unknown) {
  try {
    console.log(label + ":", fn());
  } catch (err: any) {
    console.log(label + ":", err?.name);
  }
}

show("arraybuffer clone", () => {
  const ab = new ArrayBuffer(4);
  const bytes = new Uint8Array(ab);
  bytes[0] = 11;
  bytes[3] = 44;
  const clone = structuredClone(ab);
  const cloneBytes = new Uint8Array(clone);
  return [
    ab.byteLength,
    clone.byteLength,
    clone === ab,
    cloneBytes[0],
    cloneBytes[3],
  ].join(",");
});

show("arraybuffer transfer", () => {
  const ab = new ArrayBuffer(3);
  new Uint8Array(ab)[1] = 77;
  const moved = structuredClone(ab, { transfer: [ab] });
  return [ab.byteLength, moved.byteLength, new Uint8Array(moved)[1]].join(",");
});

show("object transfer", () => {
  const ab = new ArrayBuffer(2);
  new Uint8Array(ab)[0] = 9;
  const src = { ab };
  const out = structuredClone(src, { transfer: [ab] });
  return [
    ab.byteLength,
    out.ab.byteLength,
    new Uint8Array(out.ab)[0],
    out.ab === ab,
  ].join(",");
});

show("unreachable transfer", () => {
  const ab = new ArrayBuffer(5);
  const out = structuredClone({ ok: true }, { transfer: [ab] });
  return [ab.byteLength, out.ok].join(",");
});

show("invalid transfer value", () => structuredClone({}, { transfer: [{}] }));
show("duplicate transfer", () => {
  const ab = new ArrayBuffer(1);
  return structuredClone({}, { transfer: [ab, ab] });
});
show("bad options", () => structuredClone({}, 1 as any));
show("bad transfer shape", () => structuredClone({}, { transfer: null as any }));
show("clone function", () => structuredClone(() => {}));
show("clone symbol", () => structuredClone(Symbol("x")));
