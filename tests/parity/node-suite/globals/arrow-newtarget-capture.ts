// @ts-nocheck

function show(label, value) {
  console.log(label + ":" + value);
}

function syncArrowFactory() {
  return () => new.target;
}

function asyncArrowFactory() {
  return async () => new.target;
}

function asyncAwaitArrowFactory() {
  return async () => {
    await Promise.resolve();
    return new.target;
  };
}

function ordinaryFactory() {
  return function () {
    return new.target;
  };
}

async function main() {
  const SyncCtor = function SyncCtor() {};
  const AsyncCtor = function AsyncCtor() {};
  const AsyncAwaitCtor = function AsyncAwaitCtor() {};
  const OrdinaryCtor = function OrdinaryCtor() {};

  const syncValue = Reflect.construct(syncArrowFactory, [], SyncCtor)();
  const asyncValue = await Reflect.construct(asyncArrowFactory, [], AsyncCtor)();
  const asyncAwaitValue = await Reflect.construct(asyncAwaitArrowFactory, [], AsyncAwaitCtor)();
  const ordinaryValue = Reflect.construct(ordinaryFactory, [], OrdinaryCtor)();

  show("sync arrow new.target", syncValue && syncValue.name);
  show("async arrow new.target", asyncValue && asyncValue.name);
  show("async await arrow new.target", asyncAwaitValue && asyncAwaitValue.name);
  show("ordinary function own new.target", ordinaryValue === undefined);
}

main();
