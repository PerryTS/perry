// Compare each platform with its own oracle: Node leaves self absent on the
// main thread; Bun installs a replaceable accessor for the realm global.
function attempt(label: string, fn: () => unknown) {
  try { console.log(label, fn()); }
  catch (error) { console.log(label, error instanceof ReferenceError ? 'ReferenceError' : 'other'); }
}
console.log('initial', typeof self, 'self' in globalThis);
attempt('Math', () => self.Math.max(1, 4));
attempt('identity', () => self === globalThis);
attempt('write', () => { self.__selfProbe = 7; return globalThis.__selfProbe; });
if (typeof self !== 'undefined') {
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, 'self')!;
  console.log('accessor', typeof descriptor.get, typeof descriptor.set,
    descriptor.enumerable, descriptor.configurable);
  console.log('borrowed getter', descriptor.get!.call({}) === globalThis);
}
// Both platforms must honor a user-installed value, including builtin names.
globalThis.self = { Math: { max: () => 99 }, marker: 42 };
console.log('replacement', self.Math.max(1, 4), self.marker);
console.log('computed', self['Math'].max(1, 4));
function shadow(self: any) { return self.Math.max(1, 4); }
console.log('shadow', shadow({ Math: { max: () => 88 } }));
const replacement = Object.getOwnPropertyDescriptor(globalThis, 'self')!;
console.log('data', 'value' in replacement, replacement.writable,
  replacement.enumerable, replacement.configurable);

async function later() {
  const first = await import('./_helpers/platform_self_replacement.ts');
  console.log('later replacement', first.observed);
  delete globalThis.self;
  const second = await import('./_helpers/platform_self_deleted.ts');
  console.log('later deleted', second.observed);
  attempt('deleted Math', () => self.Math.max(1, 4));
}
later();
