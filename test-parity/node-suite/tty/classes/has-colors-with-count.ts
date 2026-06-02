import tty from "node:tty";

const proto: any = tty.WriteStream.prototype;

console.log("has default depth 2:", proto.hasColors(2, {}) === true);
console.log("has default depth 16:", proto.hasColors(16, {}) === false);
console.log("has force1 16:", proto.hasColors(16, { FORCE_COLOR: "1" }) === true);
console.log("has force1 256:", proto.hasColors(256, { FORCE_COLOR: "1" }) === false);
console.log("has force2 256:", proto.hasColors(256, { FORCE_COLOR: "2" }) === true);
console.log("has force2 16m:", proto.hasColors(16777216, { FORCE_COLOR: "2" }) === false);
console.log("has force3 16m:", proto.hasColors(16777216, { FORCE_COLOR: "3" }) === true);
console.log("has env object:", proto.hasColors({ FORCE_COLOR: "2" }) === true);

function checkError(label: string, fn: () => unknown) {
  try {
    fn();
    console.log(label + ":", "no-error");
  } catch (e: any) {
    console.log(label + ":", e?.name, e?.code || "no-code");
  }
}

checkError("count below range", () => proto.hasColors(1));
checkError("count non-integer", () => proto.hasColors(2.5));
checkError("count infinity", () => proto.hasColors(Infinity));
checkError("count string", () => proto.hasColors("16"));
checkError("count null", () => proto.hasColors(null));
checkError("count object with env", () => proto.hasColors({}, {}));
