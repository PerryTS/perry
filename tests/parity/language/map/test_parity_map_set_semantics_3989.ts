function check(value: boolean, label: string) { if (!value) throw label; }

const mapClear = Object.getOwnPropertyDescriptor(Map.prototype, "clear")!;
check(typeof mapClear.value === "function" && mapClear.enumerable === false, "Map.prototype.clear descriptor");
const mapSize = Object.getOwnPropertyDescriptor(Map.prototype, "size")!;
check(typeof mapSize.get === "function", "Map.prototype.size getter");
let mapSizeThrew = false;
try { mapSize.get!.call({}); } catch { mapSizeThrew = true; }
check(mapSizeThrew, "Map.prototype.size brand check");

const setSize = Object.getOwnPropertyDescriptor(Set.prototype, "size")!;
check(typeof setSize.get === "function", "Set.prototype.size getter");
let setSizeThrew = false;
try { setSize.get!.call({}); } catch { setSizeThrew = true; }
check(setSizeThrew, "Set.prototype.size brand check");

check(Object.getPrototypeOf(new Map()) === Map.prototype, "Map instance prototype");
check(Object.getPrototypeOf(new Set()) === Set.prototype, "Set instance prototype");
check(Map.prototype.constructor === Map && Set.prototype.constructor === Set, "prototype constructors");
check(new Map([[0, 1]]).size === 1, "Map constructor inserts");
check(new Set([0]).size === 1, "Set constructor inserts");

const originalMapSet = Map.prototype.set;
let mapSetCalls = 0;
Map.prototype.set = function (_k: any, _v: any) { mapSetCalls++; return this; };
const observedMap = new Map([[1, 2], [3, 4]]);
check(mapSetCalls === 2 && observedMap.size === 0, "Map constructor observable set");
let mapClosed = false;
Map.prototype.set = function (_k: any, _v: any) { throw "map boom"; };
const mapIterable: any = {};
mapIterable[Symbol.iterator] = () => ({
  next: () => ({ done: false, value: [1, 2] }),
  return: () => { mapClosed = true; return { done: true }; },
});
try { new Map(mapIterable); } catch {}
check(mapClosed, "Map constructor iterator close");
Map.prototype.set = originalMapSet;

const originalSetAdd = Set.prototype.add;
let setAddCalls = 0;
Set.prototype.add = function (_v: any) { setAddCalls++; return this; };
const observedSet = new Set([1, 2]);
check(setAddCalls === 2 && observedSet.size === 0, "Set constructor observable add");
let setClosed = false;
Set.prototype.add = function (_v: any) { throw "set boom"; };
const setIterable: any = {};
setIterable[Symbol.iterator] = () => ({
  next: () => ({ done: false, value: 1 }),
  return: () => { setClosed = true; return { done: true }; },
});
try { new Set(setIterable); } catch {}
check(setClosed, "Set constructor iterator close");
Set.prototype.add = originalSetAdd;
console.log("ok");
