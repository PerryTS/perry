// Builtin constructor name/length reads and reflection must observe mutations.
console.log(
  JSON.stringify({
    mapNameBefore: Map.name,
    mapNameOwnBefore: Object.hasOwn(Map, "name"),
    mapNameDescriptorBefore: Object.getOwnPropertyDescriptor(Map, "name")?.value,
  }),
);
delete (Map as any).name;
console.log(
  JSON.stringify({
    mapNameAfter: Map.name,
    mapNameOwnAfter: Object.hasOwn(Map, "name"),
    mapNameDescriptorAfter: Object.getOwnPropertyDescriptor(Map, "name") ?? null,
  }),
);
Object.defineProperty(Map, "name", { value: "ReboundMap", configurable: true });
console.log(JSON.stringify({ mapNameRedefined: Map.name }));

console.log(
  JSON.stringify({
    objectLengthBefore: Object.length,
    objectLengthOwnBefore: Object.hasOwn(Object, "length"),
    objectLengthDescriptorBefore: Object.getOwnPropertyDescriptor(Object, "length")?.value,
  }),
);
delete (Object as any).length;
console.log(
  JSON.stringify({
    objectLengthAfter: Object.length,
    objectLengthOwnAfter: Object.hasOwn(Object, "length"),
    objectLengthDescriptorAfter: Object.getOwnPropertyDescriptor(Object, "length") ?? null,
  }),
);
Object.defineProperty(Object, "length", { value: 7, configurable: true });
console.log(JSON.stringify({ objectLengthRedefined: Object.length }));

const M: any = [Map][0];
delete M.name;
console.log(JSON.stringify({ dynamicMapNameAfter: M.name }));

function readShadow(Object: any) {
  return Object.length;
}
console.log(JSON.stringify({ shadowedObjectLength: readShadow({ length: 42 }) }));
console.log(JSON.stringify({ objectKeysStillIntrinsic: Object.keys({ x: 1 }) }));
console.log(
  JSON.stringify({
    parseIntName: parseInt.name,
    parseIntLength: parseInt.length,
    castParseIntLength: (parseInt as any).length,
  }),
);
