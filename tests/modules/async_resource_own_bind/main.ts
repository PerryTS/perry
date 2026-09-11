import { AsyncLocalStorage, AsyncResource } from 'node:async_hooks';

function check(ok: boolean, label: string) {
  if (!ok) throw new Error(label);
}

function checkResource(Resource: any, label: string) {
  const calls: number[] = [];
  const bound = Resource.bind((value: number) => {
    calls.push(value);
    return value + 1;
  });
  check(typeof bound === 'function', `${label}: callable`);
  check(bound(41) === 42 && calls.length === 1 && calls[0] === 41,
    `${label}: dispatch callback, not constructor`);
}

checkResource(AsyncResource, 'import');
const hooks: any = process.getBuiltinModule('async_hooks');
checkResource(hooks.AsyncResource, 'getBuiltinModule');
const alias: any = hooks.AsyncResource;
checkResource(alias, 'alias');

const storage = new AsyncLocalStorage<string>();
let captured: any;
let storageBound: any;
let snapshot: any;
const receiver = { value: 7 };
storage.run('captured', () => {
  captured = alias.bind(function(this: any, n: number) {
    return `${storage.getStore()}:${this.value + n}`;
  }, 'independent-bind-test', receiver);
  storageBound = hooks.AsyncLocalStorage.bind(() => storage.getStore());
  snapshot = hooks.AsyncLocalStorage.snapshot();
});
storage.run('caller', () => {
  check(captured(5) === 'captured:12', 'resource captures context and receiver');
  check(storageBound() === 'captured', 'AsyncLocalStorage.bind context');
  check(snapshot(() => storage.getStore()) === 'captured', 'snapshot context');
  check(storage.getStore() === 'caller', 'caller context restored');
});

function ownOverrides(fn: any) {
  let calls = 0;
  for (const key of ['bind', 'call', 'apply', 'toString']) {
    fn[key] = function(this: any, n: number) {
      check(this === fn, `${key}: receiver`);
      calls++;
      return n + 1;
    };
    check(fn[key](41) === 42, `${key}: own implementation`);
    delete fn[key];
  }
  check(calls === 4, 'all own implementations called');
  for (const key of ['bind', 'call', 'apply', 'toString']) {
    for (const value of [undefined, null, 123]) {
      fn[key] = value;
      let threw = false;
      try { fn[key](41); } catch (error) { threw = error instanceof TypeError; }
      check(threw, `${key}: non-callable must throw`);
      delete fn[key];
    }
  }
  let gets = 0;
  Object.defineProperty(fn, 'bind', {
    configurable: true,
    get() {
      check(this === fn, 'getter receiver');
      gets++;
      return function(this: any, n: number) {
        check(this === fn, 'getter result receiver');
        return n + 1;
      };
    },
  });
  check(fn.bind(41) === 42 && gets === 1, 'own accessor once');
  delete fn.bind;
}
ownOverrides(function() { throw new Error('original must not run'); });

function ordinary(fn: any) {
  const receiver = { value: 10 };
  check(fn.call(receiver, 2, 3) === 15, 'ordinary call');
  check(fn.apply(receiver, [2, 3]) === 15, 'ordinary apply array');
  const bound = fn.bind(receiver, 2);
  check(bound(3) === 15, 'ordinary bind');
  function forward(this: any, _a: number, _b: number) {
    return fn.apply(receiver, arguments);
  }
  check(forward(2, 3) === 15, 'ordinary apply arguments object');
  check(typeof fn.toString() === 'string', 'ordinary toString');
}
ordinary(function(this: any, a: number, b: number) { return this.value + a + b; });
console.log('PASS: own function methods and async context binding');
