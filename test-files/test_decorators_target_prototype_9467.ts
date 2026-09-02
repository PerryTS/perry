// #9467: a legacy decorator on an INSTANCE member receives `Class.prototype`
// as its `target` — what tsc's `__decorate([...], C.prototype, key, desc)`
// hands it — and a decorator on a STATIC member receives the constructor.
// Separately, a class constructor's own `constructor` is inherited from
// Function.prototype: `C.constructor === Function`, never `C`; only
// `C.prototype.constructor` and `(new C()).constructor` are `C`.
//
// Perry used to get both wrong in a way that cancelled (target = C and
// C.constructor = C), so the NestJS idiom
// `Reflect.defineMetadata(k, v, target.constructor)` landed on C by accident.
// Fixing either half alone broke decorator metadata; this fixture pins the
// pair. Decorator application ORDER across member kinds is deliberately not
// under test (records are collected, then printed sorted).
import "reflect-metadata";

class Repo {}

const seen = new Map<string, string>();

function describeTarget(target: any): string {
  if (target === Service) return "Service";
  if (target === Service.prototype) return "Service.prototype";
  return "other:" + typeof target;
}

function Prop(): PropertyDecorator {
  return (target: any, key: string | symbol) => {
    seen.set(
      "prop:" + String(key),
      describeTarget(target) + " ctor===Service:" + (target.constructor === Service),
    );
    Reflect.defineMetadata("via:target", "T:" + String(key), target, key);
    Reflect.defineMetadata("via:ctor", "C:" + String(key), target.constructor, key);
  };
}

function Method(): MethodDecorator {
  return (target: any, key: string | symbol, desc: PropertyDescriptor) => {
    seen.set(
      "method:" + String(key),
      describeTarget(target) +
        " ctor===Service:" +
        (target.constructor === Service) +
        " value:" +
        typeof desc.value,
    );
    Reflect.defineMetadata("via:ctor", "M:" + String(key), target.constructor, key);
  };
}

function Param(): ParameterDecorator {
  return (target: any, key: string | symbol | undefined, index: number) => {
    seen.set(
      "param:" + String(key) + ":" + index,
      describeTarget(target) + " ctor===Service:" + (target.constructor === Service),
    );
    if (key !== undefined) {
      Reflect.defineMetadata("param:via:ctor", index, target.constructor, key);
    }
  };
}

class Service {
  @Prop()
  repo!: Repo;

  @Prop()
  static counter: number = 0;

  @Method()
  run(@Param() a: Repo) {}

  @Method()
  static make(@Param() n: number) {}

  constructor(@Param() r: Repo) {}
}

class Sub extends Service {}

for (const k of [...seen.keys()].sort()) {
  console.log(k, "->", seen.get(k));
}

console.log("Service.constructor === Function", Service.constructor === Function);
console.log("Service.constructor === Service", Service.constructor === Service);
console.log("Sub.constructor === Function", Sub.constructor === Function);
console.log("typeof Service.constructor", typeof Service.constructor);
console.log("Service.constructor.name", Service.constructor.name);
console.log("Service.prototype.constructor === Service", Service.prototype.constructor === Service);
console.log("Sub.prototype.constructor === Sub", Sub.prototype.constructor === Sub);
console.log("instance.constructor === Service", new Service(new Repo()).constructor === Service);
console.log("sub instance.constructor === Sub", new Sub(new Repo()).constructor === Sub);
console.log("'constructor' in Service", "constructor" in Service);
console.log("hasOwn constructor", Object.prototype.hasOwnProperty.call(Service, "constructor"));

console.log("design:type via prototype", Reflect.getMetadata("design:type", Service.prototype, "repo") === Repo);
console.log("design:type static via class", Reflect.getMetadata("design:type", Service, "counter") === Number);
console.log("design:paramtypes ctor", Reflect.getMetadata("design:paramtypes", Service)[0] === Repo);
console.log("design:paramtypes method via prototype", Reflect.getMetadata("design:paramtypes", Service.prototype, "run")[0] === Repo);
console.log("design:paramtypes static via class", Reflect.getMetadata("design:paramtypes", Service, "make")[0] === Number);
console.log("via:target on prototype", Reflect.getMetadata("via:target", Service.prototype, "repo"));
console.log("via:ctor on class", Reflect.getMetadata("via:ctor", Service, "repo"));
console.log("via:ctor method on class", Reflect.getMetadata("via:ctor", Service, "run"));
console.log("param:via:ctor on class", Reflect.getMetadata("param:via:ctor", Service, "run"));
console.log("via:ctor inherited on Sub", Reflect.getMetadata("via:ctor", Sub, "repo"));
console.log("via:target inherited on Sub.prototype", Reflect.getMetadata("via:target", Sub.prototype, "repo"));
console.log("nothing for repo on Function", Reflect.getMetadata("via:ctor", Function, "repo"));
