// Instance identity, own surface, inherited operations, brands and subclasses.
function surface(label: string, a: any, b: any, ctor: any): void {
  const map = new Map<any, string>();
  map.set(a, "first"); map.set(b, "second");
  const weak = new WeakMap<any, string>();
  weak.set(a, "first"); weak.set(b, "second");
  console.log(label, typeof a, a instanceof ctor, a !== b,
    Object.prototype.toString.call(a), String(a));
  console.log("own", JSON.stringify(Object.keys(a)), JSON.stringify(Object.getOwnPropertyNames(a)), JSON.stringify(a));
  console.log("identity", map.size, map.get(a), map.get(b), new Set([a, b, a]).size, weak.get(a), weak.get(b));
  console.log("prototype", Object.getPrototypeOf(a) === ctor.prototype,
    JSON.stringify(Object.getOwnPropertyNames(ctor.prototype)));
}
surface("target", new EventTarget(), new EventTarget(), EventTarget);
surface("event", new Event("x"), new Event("x"), Event);
surface("custom", new CustomEvent("x"), new CustomEvent("x"), CustomEvent);
const controller = new AbortController();
surface("controller", controller, new AbortController(), AbortController);
surface("signal", controller.signal, new AbortController().signal, AbortSignal);
surface("exception", new DOMException("message", "AbortError"), new DOMException("message", "AbortError"), DOMException);
console.log("same signal", controller.signal === controller.signal);
console.log("error brand", Error.isError(new DOMException()), new DOMException() instanceof Error);
console.log("method identity", new EventTarget().dispatchEvent === new EventTarget().dispatchEvent,
  controller.abort === AbortController.prototype.abort,
  controller.signal.addEventListener === EventTarget.prototype.addEventListener);

class Bus extends EventTarget {}
class ChildBus extends Bus {}
const bus = new ChildBus();
let hits = 0;
bus.addEventListener("ping", () => { hits++; });
bus.dispatchEvent(new Event("ping"));
console.log("subclass", hits, bus instanceof EventTarget, bus instanceof Bus,
  Object.getPrototypeOf(Bus.prototype) === EventTarget.prototype,
  JSON.stringify(Object.keys(bus)), JSON.stringify(Object.getOwnPropertyNames(bus)), JSON.stringify(bus));
const detail = { token: 7 };
const event = new CustomEvent("ping", { cancelable: true, detail });
console.log("detail identity", event.detail === detail, event.detail === event.detail);
event.preventDefault();
console.log("event state", event.type, event.cancelable, event.defaultPrevented, event.returnValue);
let aborted = 0;
controller.signal.addEventListener("abort", () => { aborted++; });
controller.abort(detail);
console.log("abort state", aborted, controller.signal.aborted, controller.signal.reason === detail);
console.log("still private", JSON.stringify(Object.keys(event)), JSON.stringify(Object.getOwnPropertyNames(controller.signal)));
for (const [proto, name] of [[Event.prototype, "type"], [CustomEvent.prototype, "detail"],
  [AbortController.prototype, "signal"], [AbortSignal.prototype, "reason"], [DOMException.prototype, "code"]] as const) {
  const getter = Object.getOwnPropertyDescriptor(proto, name)!.get!;
  try { getter.call({}); console.log("invalid brand accepted"); }
  catch (error: any) { console.log("invalid brand", name, error.name); }
}
class SpecialEvent extends CustomEvent {}
const special = new SpecialEvent("special", { detail });
console.log("custom subclass", special.detail === detail, special instanceof Event,
  JSON.stringify(Object.getOwnPropertyNames(special)));
class SpecialException extends DOMException {}
const specialError = new SpecialException("m", "AbortError");
console.log("exception subclass", specialError.name, specialError.message, specialError.code,
  specialError instanceof DOMException, specialError instanceof Error, Error.isError(specialError),
  JSON.stringify(Object.keys(specialError)), JSON.stringify(Object.getOwnPropertyNames(specialError)));

const targetForTag = new EventTarget();
const tagDescriptor = Object.getOwnPropertyDescriptor(EventTarget.prototype, Symbol.toStringTag)!;
delete (EventTarget.prototype as any)[Symbol.toStringTag];
console.log("prototype controls tag", Object.prototype.toString.call(targetForTag),
  (targetForTag as any)[Symbol.toStringTag]);
Object.defineProperty(EventTarget.prototype, Symbol.toStringTag, tagDescriptor);
console.log("restored tag", Object.prototype.toString.call(targetForTag));

class FieldBus extends Bus {
  first = 10;
  second = 20;
  third = 30;
}
let fieldHits = 0;
for (let i = 0; i < 3; i++) {
  const fields: any = new FieldBus();
  fields.addEventListener("field", () => { fieldHits++; });
  for (let j = 0; j < 12; j++) fields["extra" + j] = j;
  fields.dispatchEvent(new Event("field"));
  console.log("public fields", fields.first, fields.second, fields.third, fields.extra11,
    Object.keys(fields).length, Object.getOwnPropertyNames(fields).length);
}
console.log("field listeners", fieldHits);

const originalDispatch = EventTarget.prototype.dispatchEvent;
EventTarget.prototype.dispatchEvent = function (_event: Event): boolean {
  console.log("replaced dispatch", this instanceof EventTarget);
  return false;
};
console.log("dispatch result", new EventTarget().dispatchEvent(new Event("x")));
EventTarget.prototype.dispatchEvent = originalDispatch;
const originalAbort = AbortController.prototype.abort;
AbortController.prototype.abort = function (_reason?: any): void {
  console.log("replaced abort", this instanceof AbortController);
};
new AbortController().abort();
AbortController.prototype.abort = originalAbort;
console.log("constructor chains", Object.getPrototypeOf(AbortSignal) === EventTarget,
  Object.getPrototypeOf(CustomEvent) === Event, Object.getPrototypeOf(DOMException) === Function.prototype);
const DynamicBase: any = EventTarget;
class DynamicBus extends DynamicBase {}
class DynamicChild extends DynamicBus { count = 9; }
for (let i = 0; i < 2; i++) {
  const dynamic = new DynamicChild();
  let seen = 0;
  dynamic.addEventListener("x", () => { seen++; });
  dynamic.dispatchEvent(new Event("x"));
  console.log("dynamic ancestor", seen, dynamic.count, dynamic instanceof EventTarget,
    JSON.stringify(Object.keys(dynamic)));
}
try { EventTarget.prototype.dispatchEvent.call(Bus, new Event("x")); }
catch (error: any) { console.log("constructor is not instance", error.name); }
console.log("constants", Event.NONE, Event.AT_TARGET, DOMException.ABORT_ERR,
  DOMException.prototype.ABORT_ERR, (Event.prototype as any).NONE);
const retargeted = new Event("x");
Object.setPrototypeOf(retargeted, Object.prototype);
console.log("prototype controls instanceof", retargeted instanceof Event,
  Object.create(Event.prototype) instanceof Event);
try { Object.getOwnPropertyDescriptor(Event.prototype, "type")!.get!.call(Object.create(Event.prototype)); }
catch (error: any) { console.log("prototype is not brand", error.name); }
const frozenEvent = Object.freeze(new Event("frozen", { cancelable: true }));
frozenEvent.preventDefault();
console.log("frozen private state", frozenEvent.defaultPrevented,
  JSON.stringify(Object.keys(frozenEvent)));
const accessorEvent = new Event("original", { cancelable: true });
console.log("readonly attribute", Reflect.set(accessorEvent, "type", "changed"), accessorEvent.type);
accessorEvent.cancelBubble = true;
accessorEvent.returnValue = false;
const callbackController = new AbortController();
let onAbortHits = 0;
callbackController.signal.onabort = () => { onAbortHits++; };
callbackController.abort("reason");
console.log("prototype setters", accessorEvent.cancelBubble, accessorEvent.defaultPrevented,
  onAbortHits, callbackController.signal.reason, Object.keys(callbackController.signal).length);
try {
  EventTarget.prototype.addEventListener.call(Bus.prototype, 'x', () => {});
  console.log('prototype brand', false);
} catch (e) { console.log('prototype brand', e instanceof TypeError); }
class DerivedController extends AbortController {}
const derivedController = new DerivedController();
console.log('controller subclass', derivedController instanceof DerivedController,
  derivedController instanceof AbortController, derivedController.signal instanceof AbortSignal,
  derivedController.signal === derivedController.signal,
  Object.getOwnPropertyNames(derivedController).join(','));
derivedController.abort('subclass reason');
console.log('controller subclass state', derivedController.signal.aborted, derivedController.signal.reason);
class DerivedSignal extends AbortSignal {}
try { new DerivedSignal(); console.log('signal illegal constructor', false); }
catch (e) { console.log('signal illegal constructor', e instanceof TypeError); }

function symbolSurface(label: string, object: any): void {
  console.log('symbols', label, Object.getOwnPropertySymbols(object).map(String).join(','));
  console.log('all keys', label, Reflect.ownKeys(object).map(String).join(','));
}
symbolSurface('target', new EventTarget());
symbolSurface('event', new Event('x'));
symbolSurface('custom', new CustomEvent('x'));
symbolSurface('controller', new AbortController());
symbolSurface('signal', new AbortController().signal);
symbolSurface('exception', new DOMException());
const lazyTarget: any = Object.setPrototypeOf({}, EventTarget.prototype);
symbolSurface('lazy before', lazyTarget);
let lazyHits = 0;
lazyTarget.addEventListener('lazy', () => { lazyHits++; });
lazyTarget.dispatchEvent(new Event('lazy'));
symbolSurface('lazy after', lazyTarget);
console.log('lazy state', lazyHits, Object.keys(lazyTarget).length, JSON.stringify(lazyTarget));
const symbolEvent = new Event('before');
const typeSymbol = Object.getOwnPropertySymbols(symbolEvent)[0];
(symbolEvent as any)[typeSymbol] = 'after';
console.log('symbol is state', symbolEvent.type);
symbolEvent.stopImmediatePropagation();
symbolSurface('stopped event', symbolEvent);
const publicController = new AbortController();
publicController.signal.onabort = () => {};
publicController.abort('done');
symbolSurface('controller after use', publicController);
symbolSurface('signal after use', publicController.signal);

// Node's public symbol brand and private-field brand are distinct.
const brandedType = Object.getOwnPropertySymbols(new Event('brand'))[0];
const publicBrand: any = { [brandedType]: 'borrowed' };
for (const property of ['type', 'target', 'currentTarget', 'eventPhase', 'cancelable', 'bubbles']) {
  try { console.log('borrowed getter', property, Object.getOwnPropertyDescriptor(Event.prototype, property)!.get!.call(publicBrand)); }
  catch (error: any) { console.log('borrowed getter', property, error.name); }
}
console.log('trusted getter', Object.getOwnPropertyDescriptor(Event.prototype, 'isTrusted')!.get!.call({}));
for (const method of ['preventDefault', 'stopPropagation']) {
  try { (Event.prototype as any)[method].call(publicBrand); console.log('borrowed method', method, 'accepted'); }
  catch (error: any) { console.log('borrowed method', method, error.name); }
}
console.log('frozen remains frozen', Object.isFrozen(frozenEvent));

class SpreadTarget extends EventTarget { constructor(...args: any[]) { super(...args); } }
const st = new SpreadTarget();
console.log('spread target', st instanceof SpreadTarget, st instanceof EventTarget, Object.getOwnPropertySymbols(st).map(String).join(','));
class ExplicitController extends AbortController { field = 3; constructor() { super(); } }
const ec = new ExplicitController();
ec.abort('explicit');
console.log('explicit controller',ec.field,ec.signal.aborted,ec.signal.reason);
class SpreadController extends AbortController { constructor(...args: any[]) { super(...args); } }
const sc = new SpreadController();
try { console.log('spread controller',sc.signal instanceof AbortSignal); } catch(e:any){console.log('spread controller',e.name);}
class SpreadEvent extends Event { constructor(...args:any[]) { super(...args); } }
const se = new SpreadEvent('spread', {cancelable:true});
try {console.log('spread event',se.type,se.cancelable);}catch(e:any){console.log('spread event',e.name);}
