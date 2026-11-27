import "reflect-metadata";

const target = {};
const { defineMetadata, getMetadata } = Reflect;
console.log(typeof defineMetadata, typeof getMetadata);
defineMetadata("kind", "service", target);
console.log(getMetadata("kind", target), Reflect.getOwnMetadata("kind", target));
