// `new Map` must evaluate the live global binding exactly once, before its
// arguments, while Map's internal iterable initialization keeps using the
// intrinsic Map.prototype even if the global binding changes.
const IntrinsicMap = Map;
const originalDescriptor = Object.getOwnPropertyDescriptor(globalThis, "Map");

function restoreMap(): void {
  if (originalDescriptor) {
    Object.defineProperty(globalThis, "Map", originalDescriptor);
  }
}

function installMapGetter(getter: () => any): void {
  Object.defineProperty(globalThis, "Map", {
    get: getter,
    configurable: true,
    enumerable: true,
  });
}

let reads = 0;
installMapGetter(() => {
  reads++;
  return IntrinsicMap;
});
const fromEntries = new Map([[1, 2]]);
if (reads !== 1 || fromEntries.get(1) !== 2) {
  throw new Error(`iterable construction: reads=${reads}`);
}
restoreMap();

reads = 0;
installMapGetter(() => {
  reads++;
  return IntrinsicMap;
});
const empty = new Map();
if (reads !== 1 || empty.size !== 0) {
  throw new Error(`zero-argument construction: reads=${reads}`);
}
restoreMap();

reads = 0;
installMapGetter(() => {
  reads++;
  return IntrinsicMap;
});
const nullInit = new Map(null);
if (reads !== 1 || nullInit.size !== 0) {
  throw new Error(`null construction: reads=${reads}`);
}
restoreMap();

class AlternateMap {
  marker = 7;
}
reads = 0;
installMapGetter(() => {
  reads++;
  return AlternateMap;
});
const alternate = new Map();
if (reads !== 1 || !(alternate instanceof AlternateMap) || alternate.marker !== 7) {
  throw new Error(`alternate constructor: reads=${reads}`);
}
restoreMap();

let argumentEffects = 0;
installMapGetter(() => {
  throw new Error("Map getter");
});
let getterThrew = false;
try {
  new Map((argumentEffects++, []));
} catch (error) {
  getterThrew = String(error).includes("Map getter");
}
if (!getterThrew || argumentEffects !== 0) {
  throw new Error(`constructor order: effects=${argumentEffects}`);
}
restoreMap();

reads = 0;
installMapGetter(() => {
  reads++;
  return AlternateMap;
});
const savedAlias = new IntrinsicMap([[3, 4]]);
if (reads !== 0 || savedAlias.get(3) !== 4) {
  throw new Error(`saved alias iterable: reads=${reads}`);
}
restoreMap();

reads = 0;
installMapGetter(() => {
  reads++;
  return IntrinsicMap;
});
const globalThisMap = new globalThis.Map([[5, 6]]);
if (reads !== 1 || globalThisMap.get(5) !== 6) {
  throw new Error(`globalThis.Map construction: reads=${reads}`);
}
restoreMap();

reads = 0;
const evaluationOrder: string[] = [];
installMapGetter(() => {
  reads++;
  evaluationOrder.push("constructor");
  return IntrinsicMap;
});
const spread = new Map(...[[[7, 8]]]);
if (reads !== 1 || spread.get(7) !== 8) {
  throw new Error(`spread construction: reads=${reads} size=${spread.size} value=${spread.get(7)} has=${spread.has(7)} entries=${JSON.stringify([...spread])} order=${evaluationOrder.join(",")}`);
}
restoreMap();

reads = 0;
installMapGetter(() => {
  reads++;
  evaluationOrder.push("constructor");
  return IntrinsicMap;
});
const extraArgs = new Map(
  (evaluationOrder.push("arg1"), []),
  (evaluationOrder.push("arg2"), 0),
);
if (
  reads !== 1 ||
  extraArgs.size !== 0 ||
  evaluationOrder.slice(-3).join(",") !== "constructor,arg1,arg2"
) {
  throw new Error(`extra argument order: ${evaluationOrder.join(",")}`);
}
restoreMap();

class AlternateMethods {
  get(key: number): number {
    return key + 40;
  }
  set(_key: number, value: number): number {
    return value + 80;
  }
}
reads = 0;
installMapGetter(() => {
  reads++;
  return AlternateMethods;
});
const alternateMethods = new Map();
if (reads !== 1 || alternateMethods.get(2) !== 42 || alternateMethods.set(3, 4) !== 84) {
  throw new Error(`alternate methods: reads=${reads}`);
}
restoreMap();

installMapGetter(() => AlternateMethods);
const typeArgumentsDoNotBrand = new Map<number, number>();
if (typeArgumentsDoNotBrand.get(2) !== 42 || typeArgumentsDoNotBrand.set(3, 4) !== 84) {
  throw new Error("Map type arguments incorrectly branded the constructor result");
}
restoreMap();

installMapGetter(() => AlternateMethods);
const globalThisTypeArgumentsDoNotBrand = new globalThis.Map<number, number>();
if (
  globalThisTypeArgumentsDoNotBrand.get(2) !== 42 ||
  globalThisTypeArgumentsDoNotBrand.set(3, 4) !== 84
) {
  throw new Error("globalThis.Map type arguments incorrectly branded the constructor result");
}
restoreMap();

function alternateReturningObject(): any {
  return {
    get(key: number): number {
      return key + 50;
    },
    set(_key: number, value: number): number {
      return value + 90;
    },
  };
}
installMapGetter(() => alternateReturningObject);
const returnedObject = new Map();
if (returnedObject.get(2) !== 52 || returnedObject.set(3, 4) !== 94) {
  throw new Error("constructor-returned object methods");
}
restoreMap();

const originalSetDescriptor = Object.getOwnPropertyDescriptor(IntrinsicMap.prototype, "set");
const originalSet = IntrinsicMap.prototype.set;
let setGetterReads = 0;
let setReceiverIsMap = false;
let setCalls = 0;
Object.defineProperty(IntrinsicMap.prototype, "set", {
  configurable: true,
  get() {
    setGetterReads++;
    return function (this: any, key: any, value: any): any {
      setCalls++;
      setReceiverIsMap = this instanceof IntrinsicMap;
      return originalSet.call(this, key, value);
    };
  },
});
const accessorInitialized = new IntrinsicMap([[9, 10], [11, 12]]);
if (
  setGetterReads !== 1 ||
  setCalls !== 2 ||
  !setReceiverIsMap ||
  accessorInitialized.get(9) !== 10
) {
  throw new Error(
    `Map.prototype.set getter: reads=${setGetterReads} calls=${setCalls} receiver=${setReceiverIsMap}`,
  );
}
if (originalSetDescriptor) {
  Object.defineProperty(IntrinsicMap.prototype, "set", originalSetDescriptor);
}

let iteratorGetterReads = 0;
let setGetterThrew = false;
Object.defineProperty(IntrinsicMap.prototype, "set", {
  configurable: true,
  get() {
    throw new Error("Map.prototype.set getter");
  },
});
try {
  new IntrinsicMap({
    get [Symbol.iterator]() {
      iteratorGetterReads++;
      return function* () {
        yield [1, 2];
      };
    },
  });
} catch (error) {
  setGetterThrew = String(error).includes("Map.prototype.set getter");
}
if (!setGetterThrew || iteratorGetterReads !== 0) {
  throw new Error(`Map.prototype.set throw order: iterator reads=${iteratorGetterReads}`);
}
if (originalSetDescriptor) {
  Object.defineProperty(IntrinsicMap.prototype, "set", originalSetDescriptor);
}

const chainedMap = new Map().set("a", 1).set("b", 2);
const iterated: string[] = [];
for (const [key, value] of chainedMap) {
  iterated.push(`${key}:${value}`);
}
const spreadEntries = [...chainedMap];
if (
  chainedMap.get("a") !== 1 ||
  chainedMap.get("b") !== 2 ||
  iterated.join(",") !== "a:1,b:2" ||
  spreadEntries.length !== 2
) {
  throw new Error(`dynamic Map collection controls: ${iterated}`);
}

console.log("Map constructor getter semantics passed");
