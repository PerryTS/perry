// Reserved literal capacity must not create observable keys. Exercise the
// normal key-add, descriptor and spill edges, including moving collections.
declare function gc(): void;
function collect(): void { if (typeof gc === 'function') gc(); }
function show(label: string, object: any): void {
  const seen: string[] = [];
  for (const key in object) seen.push(key);
  console.log(label, Object.keys(object).join(','), seen.join(','), JSON.stringify(object));
}
function conditional(take: boolean): any {
  const object: any = { a: 1, b: 2 };
  if (take) object.z = { value: 3 };
  collect();
  return object;
}
show('never', conditional(false));
show('taken', conditional(true));
function factory(): any { return { a: 1, b: 2 }; }
const made: any = factory();
made.z = 'factory';
collect();
show('factory', made);
const local: any = { a: 1, b: 2 };
function captured(): void { local.z = { value: 4 }; }
captured();
collect();
show('captured', local);
delete local.z;
show('deleted', local);
local.z = 5;
show('readded', local);
console.log('copy', JSON.stringify({ ...local }), JSON.stringify(Object.assign({}, local)));
const method: any = { a: 1, b: 2, run() { this.z = 6; } };
method.run();
collect();
show('method', method);
const frozen: any = { a: 1, b: 2 };
Object.freeze(frozen);
try { frozen.z = 7; } catch (_) { console.log('frozen rejected'); }
show('frozen', frozen);
const sealed: any = { a: 1, b: 2 };
Object.seal(sealed);
try { sealed.z = 7; } catch (_) { console.log('sealed rejected'); }
show('sealed', sealed);
const closed: any = { a: 1, b: 2 };
Object.preventExtensions(closed);
try { closed.z = 7; } catch (_) { console.log('closed rejected'); }
show('closed', closed);
const accessor: any = { a: 1, b: 2 };
Object.defineProperty(accessor, 'z', { get() { return 8; }, enumerable: true, configurable: true });
try { accessor.z = 9; } catch (_) { console.log('getter rejected'); }
show('accessor', accessor);
const wide: any = { a: 1, b: 2 };
wide.c = { value: 1 }; wide.d = { value: 2 }; wide.e = { value: 3 };
wide.f = { value: 4 }; wide.g = { value: 5 }; wide.h = { value: 6 };
wide.i = { value: 7 }; wide.j = { value: 8 }; wide.k = { value: 9 };
wide.l = { value: 10 }; wide.m = { value: 11 }; wide.n = { value: 12 };
collect();
show('beyond cap', wide);
console.log('children', wide.c.value, wide.j.value, wide.n.value);
