// Named array accessor merge semantics through Object/Reflect and no-trap proxies.
for (const proxied of [false, true]) {
  for (const reflect of [false, true]) {
    const array: any = [];
    const target: any = proxied ? new Proxy(array, {}) : array;
    let value = 17;
    const getter = function (this: any) { return value; };
    const setter = function (this: any, next: number) { value = next; };
    const define = (descriptor: any) => {
      if (reflect) {
        if (!Reflect.defineProperty(target, "named", descriptor)) throw new Error("rejected");
      } else Object.defineProperty(target, "named", descriptor);
    };
    define({ get: getter, set: setter, enumerable: true, configurable: true });
    target.named = 29;
    if (target.named !== 29) throw new Error("pair behavior");
    const priorSetter = Object.getOwnPropertyDescriptor(array, "named")!.set;
    define({ get: getter });
    if (Object.getOwnPropertyDescriptor(array, "named")!.set !== priorSetter) throw new Error("setter retention");
    const priorGetter = Object.getOwnPropertyDescriptor(array, "named")!.get;
    define({ set: setter });
    if (Object.getOwnPropertyDescriptor(array, "named")!.get !== priorGetter) throw new Error("getter retention");
    define({ enumerable: false });
    if (target.named !== 29) throw new Error("generic retention");
    define({ get: undefined });
    if (target.named !== undefined) throw new Error("present undefined getter");
    target.named = 31;
    define({ get: getter, set: undefined });
    if (target.named !== 31) throw new Error("getter restore");
    const record = Object.getOwnPropertyDescriptor(array, "named")!;
    if (record.enumerable || !record.configurable || record.set !== undefined) throw new Error("attribute retention");
    console.log(proxied, reflect, target.named, record.enumerable, record.configurable, record.set === undefined);
  }
}
