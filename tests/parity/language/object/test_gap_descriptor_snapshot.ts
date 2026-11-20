const log = (label: string, value: any) => console.log(label, JSON.stringify(value));
const target: any = {};
const first: any = { value: { token: 'saved' }, writable: true };
const bag: any = { a: first };
Object.defineProperty(bag, 'b', { enumerable: true, get() {
  first.value = { token: 'mutated' };
  first.writable = false;
  return { value: 2 };
}});
Object.defineProperties(target, bag);
log('snapshot', [target.a.token, Object.getOwnPropertyDescriptor(target, 'a')!.writable]);

const order: string[] = [];
const fields: any = new Proxy({ enumerable: 1, configurable: 0, value: 7, writable: '' }, {
  has(t, k) { order.push('has:' + String(k)); return Reflect.has(t, k); },
  get(t, k, r) { order.push('get:' + String(k)); return Reflect.get(t, k, r); }
});
Object.defineProperties({}, { a: fields });
log('fields', order);

const invalidEvents: string[] = [];
const invalidFields: any = new Proxy({ value: 1, get: 2, set: () => {} }, {
  has(t, k) { invalidEvents.push('has:' + String(k)); return Reflect.has(t, k); },
  get(t, k, r) { invalidEvents.push('get:' + String(k)); return Reflect.get(t, k, r); }
});
try { Object.defineProperty({}, 'x', invalidFields); } catch(e) { invalidEvents.push('TypeError:' + (e instanceof TypeError)); }
log('getter-validation', invalidEvents);

const symbol = Symbol('late');
const mutationBag: any = {};
Object.defineProperty(mutationBag, 'a', { enumerable: true, get() {
  Object.defineProperty(mutationBag, 'b', { enumerable: false });
  mutationBag.c = { value: 3 };
  mutationBag[symbol] = { value: 4 };
  return { value: 1, enumerable: true };
}});
Object.defineProperty(mutationBag, 'b', { value: { value: 2 }, enumerable: true, configurable: true });
const mutationTarget: any = {};
Object.defineProperties(mutationTarget, mutationBag);
log('ownkeys-snapshot-current-enumerability', [Reflect.ownKeys(mutationTarget), Object.getOwnPropertySymbols(mutationTarget).length]);

const inherited = Object.create({ value: 9, writable: true, enumerable: true });
const inheritedTarget: any = {};
Object.defineProperties(inheritedTarget, { x: inherited });
log('inherited-fields', Object.getOwnPropertyDescriptor(inheritedTarget, 'x'));

const originalTarget: any = {};
const trapBags: any[] = [];
const proxyTarget = new Proxy(originalTarget, { defineProperty(t, k, d) {
  trapBags.push([String(k), Reflect.ownKeys(d), typeof d.enumerable, d.enumerable]);
  d.value = 100;
  return true;
}});
const editable: any = { value: 1, enumerable: 'yes' };
Object.defineProperties(proxyTarget, { a: editable });
log('proxy-normalized-copy', [trapBags, editable.value, editable.enumerable]);

const invariantTarget = Object.defineProperty({}, 'x', { value: 1, writable: false, configurable: false });
let invariantError = false;
try { Object.defineProperty(new Proxy(invariantTarget, { defineProperty(t, k, d) { d.value = 1; return true; } }), 'x', { value: 2 }); }
catch(e) { invariantError = e instanceof TypeError; }
log('proxy-invariant-original-facts', invariantError);

const partial: any = Object.defineProperty({}, 'b', { value: 0, configurable: false });
const decodeEvents: string[] = [];
const partialBag: any = {};
for (const key of ['a', 'b', 'c']) Object.defineProperty(partialBag, key, { enumerable: true, get() { decodeEvents.push(key); return { value: key }; } });
let definitionError = false;
try { Object.defineProperties(partial, partialBag); } catch(e) { definitionError = e instanceof TypeError; }
log('definition-partial', [decodeEvents, definitionError, partial.a, partial.b, Object.hasOwn(partial, 'c')]);

const userEffects: any = {};
const failureBag: any = {};
Object.defineProperty(failureBag, 'a', { enumerable: true, get() { userEffects.user = 1; return { value: 2 }; } });
failureBag.b = 3;
try { Object.defineProperties(userEffects, failureBag); } catch(e) {}
log('collection-error-user-effects', [userEffects.user, Object.hasOwn(userEffects, 'a')]);

const typed: any = new Uint8Array(1);
let numericCalls = 0;
const numericValue = { valueOf() { numericCalls++; return 11; } };
const typedFirst: any = { value: numericValue, writable: true, enumerable: true, configurable: true };
const typedBag: any = { '0': typedFirst };
Object.defineProperty(typedBag, 'tag', { enumerable: true, get() { typedFirst.value = 29; return { value: 'saved' }; } });
Object.defineProperties(typed, typedBag);
log('typed-snapshot-coercion', [typed[0], numericCalls, typed.tag]);

const assignmentEvents: string[] = [];
const setterProto = { set a(v: number) { assignmentEvents.push('set:a:' + v); } };
const setterTarget = Object.create(setterProto);
const source: any = { get a() { assignmentEvents.push('get:a'); return 1; }, get b() { assignmentEvents.push('get:b'); return 2; } };
Object.assign(setterTarget, source);
log('assign-set-order', [assignmentEvents, Object.hasOwn(setterTarget, 'a'), setterTarget.b]);
const spread = { ...source };
log('spread-own-definitions', [Object.hasOwn(spread, 'a'), Object.getOwnPropertyDescriptor(spread, 'a')!.writable]);
