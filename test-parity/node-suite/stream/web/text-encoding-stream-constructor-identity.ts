for (const name of ["TextEncoderStream", "TextDecoderStream"]) {
  const Ctor = (globalThis as any)[name];
  const instance = new Ctor();
  console.log(`${name} typeof:`, typeof Ctor);
  console.log(`${name} name:`, Ctor.name);
  console.log(`${name} length:`, Ctor.length);
  console.log(`${name} constructor identity:`, instance.constructor === Ctor);
  console.log(`${name} instanceof:`, instance instanceof Ctor);
  console.log(`${name} prototype constructor:`, Object.getPrototypeOf(instance)?.constructor === Ctor);
  console.log(`${name} has readable:`, "readable" in instance);
  console.log(`${name} has writable:`, "writable" in instance);
}
