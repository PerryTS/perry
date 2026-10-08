// The side-channel pattern qs's stringify runs per key: a factory returning an
// object of short closures over shared mutable locals, a lazily created inner
// channel, and uncurried WeakMap methods. Pin its behaviour (identities,
// sharing of the captured state, cycle detection) against Node.

const uncurry = (fn: Function): any =>
  Reflect.apply(Function.prototype.bind, Function.prototype.call, [fn]);
const $weakMapGet = uncurry(WeakMap.prototype.get);
const $weakMapSet = uncurry(WeakMap.prototype.set);
const $weakMapHas = uncurry(WeakMap.prototype.has);

function getSideChannelList() {
  let $o: { key: unknown; value: unknown }[] | undefined;
  const channel = {
    get(key: unknown) {
      if (!$o) return undefined;
      for (const e of $o) if (e.key === key) return e.value;
      return undefined;
    },
    has(key: unknown) {
      return !!$o && $o.some((e) => e.key === key);
    },
    set(key: unknown, value: unknown) {
      if (!$o) $o = [];
      for (const e of $o) {
        if (e.key === key) {
          e.value = value;
          return;
        }
      }
      $o.push({ key, value });
    },
  };
  return channel;
}

function getSideChannelWeakMap() {
  let $wm: WeakMap<object, unknown> | undefined;
  let $m: ReturnType<typeof getSideChannelList> | undefined;
  const channel = {
    assert(key: unknown) {
      if (!channel.has(key)) throw new TypeError("Side channel does not contain key");
    },
    get(key: any) {
      if (key && (typeof key === "object" || typeof key === "function")) {
        if ($wm) return $weakMapGet($wm, key);
      }
      return $m && $m.get(key);
    },
    has(key: any) {
      if (key && (typeof key === "object" || typeof key === "function")) {
        if ($wm) return $weakMapHas($wm, key);
      }
      return !!$m && $m.has(key);
    },
    set(key: any, value: unknown) {
      if (key && (typeof key === "object" || typeof key === "function")) {
        if (!$wm) $wm = new WeakMap();
        $weakMapSet($wm, key, value);
      } else {
        if (!$m) $m = getSideChannelList();
        $m.set(key, value);
      }
    },
  };
  return channel;
}

function getSideChannel() {
  let $channelData: ReturnType<typeof getSideChannelWeakMap> | undefined;
  const channel = {
    assert(key: unknown) {
      if (!channel.has(key)) throw new TypeError("Side channel does not contain key");
    },
    get(key: unknown) {
      return $channelData && $channelData.get(key);
    },
    has(key: unknown) {
      return !!$channelData && $channelData.has(key);
    },
    set(key: unknown, value: unknown) {
      if (!$channelData) $channelData = getSideChannelWeakMap();
      $channelData.set(key, value);
    },
  };
  return channel;
}

// Identity: every factory call mints its own closures, sharing nothing.
const c1 = getSideChannel();
const c2 = getSideChannel();
console.log(c1.get === c2.get, c1.set === c1.set, typeof c1.assert, Object.keys(c1).join(","));

// The closures share their channel's state, and only theirs.
const k1 = {};
c1.set(k1, "one");
c1.set("prim", 42);
console.log(c1.get(k1), c1.has(k1), c2.has(k1), c1.get("prim"), c2.get("prim"));
try {
  c2.assert(k1);
} catch (err) {
  console.log("assert", (err as Error).message);
}

// Detached method values keep working with their own channel.
const { get, set, has } = getSideChannel();
const k2 = { z: 1 };
set(k2, "detached");
console.log(get(k2), has(k2), has({}));

// qs-like recursive walk: a fresh channel per key, linked through a sentinel,
// used to detect cycles.
const sentinel = {};
function walk(obj: any, prefix: string, sideChannel: ReturnType<typeof getSideChannel>, out: string[]): void {
  let tmpSc: any = sideChannel;
  let step = 0;
  let findFlag = false;
  while ((tmpSc = tmpSc.get(sentinel)) !== undefined && !findFlag) {
    const pos = tmpSc.get(obj);
    step += 1;
    if (typeof pos !== "undefined") {
      if (pos === step) throw new RangeError("Cyclic object value");
      findFlag = true;
    }
    if (typeof tmpSc.get(sentinel) === "undefined") step = 0;
  }
  if (obj === null || typeof obj !== "object") {
    out.push(prefix + "=" + String(obj));
    return;
  }
  for (const key of Object.keys(obj)) {
    sideChannel.set(obj, step);
    const valueSideChannel = getSideChannel();
    valueSideChannel.set(sentinel, sideChannel);
    walk(obj[key], prefix ? prefix + "[" + key + "]" : key, valueSideChannel, out);
  }
}

let checksum = 0;
for (let i = 0; i < 3000; i++) {
  const obj = {
    user: { name: "alice " + (i % 97), roles: ["admin", "dev"], meta: { a: i % 7, b: "x&y=z" } },
    filter: { age: { gte: 18, lte: 65 }, status: ["active", "pending"] },
    page: { size: 20, number: i % 50 },
  };
  const out: string[] = [];
  walk(obj, "", getSideChannel(), out);
  const s = out.join("&");
  for (let j = 0; j < s.length; j++) checksum = (checksum * 31 + s.charCodeAt(j)) >>> 0;
  if (i === 0) console.log(s);
}
console.log("checksum", checksum);

const cyclic: any = { a: { b: {} } };
cyclic.a.b.c = cyclic;
try {
  walk(cyclic, "", getSideChannel(), []);
  console.log("no cycle detected");
} catch (err) {
  console.log((err as Error).constructor.name, (err as Error).message);
}
