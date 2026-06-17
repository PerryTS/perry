// parity-node-argv: --no-warnings --permission --allow-fs-read=. --allow-addons
// parity-argv: --no-warnings --permission --allow-fs-read=. --allow-addons

function showError(label: string, fn: () => unknown) {
  try {
    console.log(label + ":", fn());
  } catch (e: any) {
    console.log(label + ":", e.name + ":" + e.code + ":" + e.message);
  }
}

const permission = process.permission;
console.log("permission typeof:", typeof permission);

if (permission && typeof permission.has === "function") {
  const descriptor = Object.getOwnPropertyDescriptor(process, "permission");
  console.log(
    "permission descriptor:",
    [
      descriptor?.enumerable,
      descriptor?.configurable,
      descriptor?.writable,
      descriptor?.value === permission,
    ].join(","),
  );
  console.log("permission identity:", process.permission === permission);
  console.log("permission key:", Object.keys(process).includes("permission"));
  console.log("permission names:", Object.getOwnPropertyNames(permission).join(","));
  console.log("has typeof:", typeof permission.has);
  console.log("has length:", permission.has.length);
  console.log("drop typeof:", typeof permission.drop);
  console.log("drop length:", permission.drop.length);
  console.log("fs.read:", permission.has("fs.read"));
  console.log("fs.read dot:", permission.has("fs.read", "."));
  console.log("fs.read buffer:", permission.has("fs.read", Buffer.from(".")));
  console.log("fs.write dot:", permission.has("fs.write", "."));
  console.log("child:", permission.has("child"));
  console.log("addon:", permission.has("addon"));
  console.log("addons:", permission.has("addons"));
  console.log("unknown:", permission.has("bad.scope"));
  console.log("drop fs.read dot:", permission.drop("fs.read", "."));
  console.log("fs.read dot after drop:", permission.has("fs.read", "."));
  console.log("drop addon:", permission.drop("addon"));
  console.log("addon after drop:", permission.has("addon"));
  showError("missing scope", () => permission.has());
  showError("bad scope type", () => permission.has(1 as any));
  showError("bad reference type", () => permission.has("fs.read", 1 as any));
} else {
  console.log("permission descriptor: unavailable");
  console.log("permission names: unavailable");
  console.log("has typeof: unavailable");
  console.log("has length: unavailable");
  console.log("drop typeof: unavailable");
  console.log("drop length: unavailable");
  console.log("fs.read: unavailable");
  console.log("fs.read dot: unavailable");
  console.log("fs.read buffer: unavailable");
  console.log("fs.write dot: unavailable");
  console.log("child: unavailable");
  console.log("addon: unavailable");
  console.log("addons: unavailable");
  console.log("unknown: unavailable");
  console.log("drop fs.read dot: unavailable");
  console.log("fs.read dot after drop: unavailable");
  console.log("drop addon: unavailable");
  console.log("addon after drop: unavailable");
  console.log("missing scope: unavailable");
  console.log("bad scope type: unavailable");
  console.log("bad reference type: unavailable");
}
