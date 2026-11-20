class Receiver {
  run(value: any): string {
    return String(value);
  }
}

function invoke(instance: any, args: any[]): string {
  return instance.run(...args);
}

const throwing: any = {};
throwing[Symbol.iterator] = function(): any {
  throw new Error("iterator-boom");
};

invoke(new Receiver(), throwing);
