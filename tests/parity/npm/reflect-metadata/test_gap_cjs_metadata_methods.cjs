require("reflect-metadata");

const key = Symbol("kind");
const target = {};
Reflect.defineMetadata(key, "service", target);
console.log(typeof Reflect.defineMetadata, Reflect.getMetadata(key, target));
