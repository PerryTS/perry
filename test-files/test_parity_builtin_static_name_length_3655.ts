let failures = 0;
function check(label: string, actual: any, expected: any): void {
  if (actual !== expected) { console.log(label, actual, expected); failures++; }
}
function checkDataDescriptor(label: string, desc: any, value: any): void {
  if (desc === undefined) { failures++; return; }
  check(label + ".value", desc.value, value);
  check(label + ".writable", desc.writable, false);
  check(label + ".enumerable", desc.enumerable, false);
  check(label + ".configurable", desc.configurable, true);
}
checkDataDescriptor("Array.from.name", Object.getOwnPropertyDescriptor(Array.from, "name"), "from");
checkDataDescriptor("Array.from.length", Object.getOwnPropertyDescriptor(Array.from, "length"), 1);
check("BigInt.asIntN.length", BigInt.asIntN.length, 2);
check("Reflect.apply.name", Reflect.apply.name, "apply");
check("Reflect.apply.length", Reflect.apply.length, 3);
checkDataDescriptor("Reflect.apply.length", Object.getOwnPropertyDescriptor(Reflect.apply, "length"), 3);
checkDataDescriptor("Math.max.length", Object.getOwnPropertyDescriptor(Math.max, "length"), 2);
checkDataDescriptor("ArrayBuffer.isView.name", Object.getOwnPropertyDescriptor(ArrayBuffer.isView, "name"), "isView");
checkDataDescriptor("Array.name", Object.getOwnPropertyDescriptor(Array, "name"), "Array");
checkDataDescriptor("Array.length", Object.getOwnPropertyDescriptor(Array, "length"), 1);
checkDataDescriptor("Map.name", Object.getOwnPropertyDescriptor(Map, "name"), "Map");
checkDataDescriptor("Map.length", Object.getOwnPropertyDescriptor(Map, "length"), 0);
checkDataDescriptor("Date.name", Object.getOwnPropertyDescriptor(Date, "name"), "Date");
checkDataDescriptor("Date.length", Object.getOwnPropertyDescriptor(Date, "length"), 7);
checkDataDescriptor("DataView.name", Object.getOwnPropertyDescriptor(DataView, "name"), "DataView");
checkDataDescriptor("DataView.length", Object.getOwnPropertyDescriptor(DataView, "length"), 1);
if (failures !== 0) throw new Error("builtin name/length descriptor parity failed");
console.log("builtin name/length descriptor parity ok");
