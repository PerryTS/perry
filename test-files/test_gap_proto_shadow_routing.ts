class C {
    m() { return 1; }
    n() { return 2; }
}
function invoke(value: any) { return value.m(); }
const key = process.argv[2] || "m";
const changed: any = new C();
const other: any = new C();
console.log(invoke(changed), invoke(other));
changed[key] = () => 3;
console.log(invoke(changed), invoke(other));
(C.prototype as any)[key] = () => 4;
console.log(invoke(changed), invoke(other));
delete changed[key];
console.log(invoke(changed), invoke(other));
Object.defineProperties(C.prototype, { [key]: { value: () => 5, configurable: true } });
console.log(invoke(changed), invoke(other), other.n());
class Holder { data = 7; }
const holder: any = new Holder();
const inherited: any = Object.create(holder);
console.log(inherited.data);
const data = process.argv[3] || "data";
delete holder[data];
console.log(inherited.data);
const proto: any = { pa: 11 };
const object: any = Object.create(proto);
console.log(object.pa);
const property = process.argv[4] || "pa";
object[property] = 12;
console.log(object.pa);
delete object[property];
console.log(object.pa);
Object.defineProperties(proto, { [property]: { get: () => 13, configurable: true } });
console.log(object.pa);

// Two names have the same length and first six bytes in the image index.
class Collision {
    colliderAlpha() { return 21; }
    colliderBravo() { return 22; }
}
const collision: any = new Collision();
const collisionKey = process.argv[5] || "colliderBravo";
console.log(collision.colliderAlpha(), collision.colliderBravo());
(Collision.prototype as any)[collisionKey] = () => 23;
console.log(collision.colliderAlpha(), collision.colliderBravo());

// A default-family answer must notice a newly inherited setter.
const defaultKey = process.argv[6] || "sharedDefault";
Object.defineProperty(Object.prototype, defaultKey, {
    set(value: number) { (this as any).savedDefault = value + 1; },
    configurable: true
});
const defaultReceiver: any = {};
defaultReceiver[defaultKey] = 30;
console.log(defaultReceiver.savedDefault, Object.hasOwn(defaultReceiver, defaultKey));
delete (Object.prototype as any)[defaultKey];

// A typed direct-call receiver must withdraw only after its own dynamic add.
class TypedReceiver { ownCall() { return 31; } }
const typedReceiver: TypedReceiver = new TypedReceiver();
const typedKey = process.argv[7] || "ownCall";
console.log(typedReceiver.ownCall());
(typedReceiver as any)[typedKey] = () => 32;
console.log(typedReceiver.ownCall());
