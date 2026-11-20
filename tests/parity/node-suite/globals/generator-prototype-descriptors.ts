// Generator and AsyncGenerator prototype method descriptors plus brand checks.

function valueShape(value: any): string {
  if (typeof value === "function") {
    return "function:" + value.name + ":" + value.length;
  }
  if (value && typeof value === "object") {
    return "object:" + Object.prototype.toString.call(value);
  }
  return typeof value + ":" + String(value);
}

function showDesc(label: string, obj: any, key: any) {
  const desc = Object.getOwnPropertyDescriptor(obj, key);
  if (!desc) {
    console.log(label + ": missing");
    return;
  }
  if ("value" in desc) {
    console.log(
      label +
        ": data:" +
        valueShape(desc.value) +
        ":" +
        desc.writable +
        ":" +
        desc.enumerable +
        ":" +
        desc.configurable,
    );
    return;
  }
  console.log(
    label +
      ": accessor:" +
      typeof desc.get +
      ":" +
      typeof desc.set +
      ":" +
      desc.enumerable +
      ":" +
      desc.configurable,
  );
}

function showMethod(prefix: string, proto: any, name: string) {
  showDesc(prefix + "." + name, proto, name);
  const method = proto[name];
  showDesc(prefix + "." + name + ".name", method, "name");
  showDesc(prefix + "." + name + ".length", method, "length");
}

function showSyncBad(proto: any, name: string, recv: any) {
  try {
    proto[name].call(recv, "x");
    console.log("sync bad " + name + ": ok");
  } catch (e) {
    console.log("sync bad " + name + ":", e instanceof TypeError);
  }
}

async function showAsyncBad(proto: any, name: string, recv: any) {
  try {
    const result = proto[name].call(recv, "x");
    const thenType = result && typeof result.then;
    try {
      await result;
      console.log("async bad " + name + ": resolved:" + thenType);
    } catch (e) {
      console.log(
        "async bad " + name + ": rejected:" + thenType + ":" + (e instanceof TypeError),
      );
    }
  } catch (e) {
    console.log("async bad " + name + ": threw:" + (e instanceof TypeError));
  }
}

async function main() {
  function* g() {
    yield 1;
  }
  async function* ag() {
    yield 1;
  }

  const GeneratorPrototype = Object.getPrototypeOf(g).prototype;
  const AsyncGeneratorPrototype = Object.getPrototypeOf(ag).prototype;

  console.log(
    "GeneratorPrototype names:",
    Object.getOwnPropertyNames(GeneratorPrototype).join(","),
  );
  for (const name of ["next", "return", "throw"]) {
    showMethod("GeneratorPrototype", GeneratorPrototype, name);
  }
  showDesc("GeneratorPrototype.constructor", GeneratorPrototype, "constructor");
  for (const name of ["next", "return", "throw"]) {
    showSyncBad(GeneratorPrototype, name, {});
  }
  for (const name of ["next", "return", "throw"]) {
    showSyncBad(GeneratorPrototype, name, g.prototype);
  }

  console.log(
    "AsyncGeneratorPrototype names:",
    Object.getOwnPropertyNames(AsyncGeneratorPrototype).join(","),
  );
  for (const name of ["next", "return", "throw"]) {
    showMethod("AsyncGeneratorPrototype", AsyncGeneratorPrototype, name);
  }
  showDesc(
    "AsyncGeneratorPrototype.constructor",
    AsyncGeneratorPrototype,
    "constructor",
  );
  for (const name of ["next", "return", "throw"]) {
    await showAsyncBad(AsyncGeneratorPrototype, name, {});
  }
  for (const name of ["next", "return", "throw"]) {
    await showAsyncBad(AsyncGeneratorPrototype, name, ag.prototype);
  }
}

main();
