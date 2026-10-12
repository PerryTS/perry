import "reflect-metadata";
const owner: any = {};
const child = { payload: 37 };
Reflect.defineMetadata("meta", child, owner);
Reflect.defineMetadata("meta", 42, owner, "member");
Reflect.defineMetadata("empty", undefined, owner);
for (let i = 0; i < 30000; i++) {
  const pressure = { i, data: [i, i + 1] };
  if (pressure.i < 0) console.log(pressure);
}
console.log(Reflect.getOwnMetadata("meta", owner).payload);
console.log(Reflect.getOwnMetadata("meta", owner, "member"));
console.log(Reflect.hasOwnMetadata("empty", owner), Reflect.hasOwnMetadata("meta", {}));
console.log(Reflect.getOwnMetadataKeys(owner).sort().join(","));
console.log(Reflect.deleteMetadata("meta", owner), Reflect.hasOwnMetadata("meta", owner));
console.log(Object.keys(owner).length);
