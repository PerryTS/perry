// @ts-nocheck

function dump(label, gen) {
  console.log(label + ":" + JSON.stringify(Array.from(gen)));
}

const sum = function* (arg) {
  yield this.foo;
  yield arg;
};

const sumProxy = new Proxy(new Proxy(sum, {}), { apply: undefined });
dump("proxy", Reflect.apply(sumProxy, { foo: 10 }, [1]));

function* named(arg) {
  yield this.foo;
  yield arg;
}

dump("direct", Reflect.apply(named, { foo: 11 }, [2]));

const receiver = {
  foo: 12,
  g: function* (arg) {
    yield this.foo;
    yield arg;
  },
};

dump("method", receiver.g(3));
