function show(label: string, fn: () => unknown) {
  try {
    console.log(label, "ok", fn());
  } catch (err: any) {
    console.log(label, err?.name, err?.message);
  }
}

const MapAlias: any = Map;
const SetAlias: any = Set;
const WeakMapAlias: any = WeakMap;
const WeakSetAlias: any = WeakSet;

show("Map direct", () => (Map as any)());
show("Map alias", () => MapAlias());
show("Map global", () => (globalThis.Map as any)());

show("Set direct", () => (Set as any)());
show("Set alias", () => SetAlias());
show("Set global", () => (globalThis.Set as any)());

show("WeakMap direct", () => (WeakMap as any)());
show("WeakMap alias", () => WeakMapAlias());
show("WeakMap global", () => (globalThis.WeakMap as any)());

show("WeakSet direct", () => (WeakSet as any)());
show("WeakSet alias", () => WeakSetAlias());
show("WeakSet global", () => (globalThis.WeakSet as any)());
