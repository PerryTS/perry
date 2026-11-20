// #11006: an own value shadows a native method even when it is not callable.
// The guard in #10943 sends these calls to the universal dispatcher; that
// dispatcher must throw rather than run the Map/Set/Date/Array builtin.
function call(label: string, fn: () => unknown) {
  try {
    console.log(label + "=" + String(fn()));
  } catch (error) {
    console.log(label + "=throw:" + (error as Error).constructor.name);
  }
}

const map = new Map<string, unknown>([["k", "native"]]);
for (const value of [1, undefined, null, "nope", {}]) {
  (map as any).get = value;
  call("map.get " + String(value), () => map.get("k"));
}
delete (map as any).get;
call("map.get after delete", () => map.get("k"));

const set = new Set(["v"]);
(set as any).has = "nope";
call("set.has", () => set.has("v"));

const date = new Date(0);
(date as any).getTime = 0;
call("date.getTime", () => date.getTime());

const array = [1];
(array as any).push = false;
call("array.push", () => array.push(2));

// A borrowed native method must still work on its original receiver.
(map as any).get = Map.prototype.get;
call("map.get borrowed", () => map.get("k"));
