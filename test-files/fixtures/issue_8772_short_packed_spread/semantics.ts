class Receiver {
  run(prefix: string, value: any = "default"): string {
    return prefix + "=" + String(value);
  }
}

class WideReceiver {
  run(prefix: string, a: any, b: any, c: any, d: any, e: any): string {
    return prefix + "=" + [a, b, c, d, e].join(",");
  }
}

class Tail extends Array<any> {}

function invoke(instance: any, prefix: string, args: any[]): string {
  return instance.run(prefix, ...args);
}

function invokeWide(instance: any, prefix: string, args: any[]): string {
  return instance.run(prefix, ...args);
}

const result: Record<string, string> = {};
const receiver = new Receiver();

result.empty = invoke(receiver, "empty", []);
result.one = invoke(receiver, "one", [1]);

const hole = new Array(1);
result.hole = invoke(receiver, "hole", hole);

const accessor: any[] = ["first", "discarded"];
Object.defineProperty(accessor, "0", {
  configurable: true,
  get: function() {
    accessor.length = 1;
    return "getter";
  }
});
result.accessorMutation = invoke(receiver, "accessor", accessor);

const own: any[] = ["own-iterator"];
own[Symbol.iterator] = own.values;
result.ownIterator = invoke(receiver, "own", own);

const proxied: any = new Proxy(["proxy"], {});
result.proxy = invoke(receiver, "proxy", proxied);

const subclass = new Tail();
subclass.push("subclass");
result.subclass = invoke(receiver, "subclass", subclass);

const mutated = ["old", "discarded"];
mutated[0] = "new";
mutated.length = 1;
result.elementAndLengthMutation = invoke(receiver, "mutated", mutated);

const replaced: any = new Receiver();
replaced.run = function(prefix: string, value: any): string {
  return "replacement=" + prefix + ":" + value;
};
result.replacedMethod = invoke(replaced, "method", [9]);

const plain: any = {
  run: function(prefix: string, value: any): string {
    return "plain=" + prefix + ":" + value;
  }
};
result.wrongReceiver = invoke(plain, "receiver", [8]);

result.oversized = invokeWide(
  new WideReceiver(),
  "wide",
  [1, 2, 3, 4, 5]
);

console.log(JSON.stringify(result));
