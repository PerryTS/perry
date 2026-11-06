import "reflect-metadata";

class Target {}

// 1. defineMetadata + getMetadata roundtrip
Reflect.defineMetadata("key1", "value1", Target);
console.log("1:", Reflect.getMetadata("key1", Target));

// 2. With propertyKey
Reflect.defineMetadata("key2", 42, Target.prototype, "method");
console.log("2:", Reflect.getMetadata("key2", Target.prototype, "method"));

// 3. hasMetadata
console.log("3:", Reflect.hasMetadata("key1", Target));
console.log("4:", Reflect.hasMetadata("nope", Target));

// 5. Decorator factory pattern
function Get(path: string) {
  return function (target: any, propertyKey: any, _descriptor: any) {
    Reflect.defineMetadata("http:path", path, target, propertyKey);
  };
}

class Controller {
  @Get("/ping")
  ping() {
    return "pong";
  }
}

console.log("5:", Reflect.getMetadata("http:path", Controller.prototype, "ping"));
