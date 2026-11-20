function show(label: string, value: any) {
  let rendered: string;
  try {
    const valueText =
      typeof value === "number" && Object.is(value, -0) ? "-0" : String(value);
    rendered = typeof value + ":" + valueText;
  } catch (err: any) {
    rendered = "render-throw:" + err.name + ":" + err.message;
  }
  console.log(label + ":", rendered);
}

function showWrapper(label: string, value: any, Ctor: any) {
  const obj = Object(value);
  console.log(label + " typeof:", typeof obj);
  console.log(label + " tag:", Object.prototype.toString.call(obj));
  show(label + " valueOf", obj.valueOf());
  console.log(label + " ctor:", obj.constructor === Ctor);
  console.log(label + " proto:", Object.getPrototypeOf(obj) === Ctor.prototype);
  console.log(label + " instanceof:", obj instanceof Ctor);
  const desc = Object.getOwnPropertyDescriptor(obj, "length");
  console.log(
    label + " own length:",
    Object.prototype.hasOwnProperty.call(obj, "length"),
    desc ? desc.value : undefined,
  );
}

showWrapper("Object true", true, Boolean);
showWrapper("Object zero", 0, Number);
showWrapper("Object string", "abc", String);

const strObj = new String("abc");
console.log("new String typeof:", typeof strObj);
console.log("new String tag:", Object.prototype.toString.call(strObj));
show("new String valueOf", strObj.valueOf());
console.log("new String length:", strObj.length);
show("new String index 0", (strObj as any)[0]);
const lengthDesc = Object.getOwnPropertyDescriptor(strObj, "length");
console.log(
  "new String length desc:",
  lengthDesc
    ? [
        lengthDesc.value,
        lengthDesc.writable,
        lengthDesc.enumerable,
        lengthDesc.configurable,
      ].join(",")
    : "missing",
);
const indexDesc = Object.getOwnPropertyDescriptor(strObj, "0");
console.log(
  "new String index desc:",
  indexDesc
    ? [
        indexDesc.value,
        indexDesc.writable,
        indexDesc.enumerable,
        indexDesc.configurable,
      ].join(",")
    : "missing",
);
console.log("new String names:", Object.getOwnPropertyNames(strObj).join(","));
console.log("new String keys:", Object.keys(strObj).join(","));
console.log("new String proto:", Object.getPrototypeOf(strObj) === String.prototype);

const sloppySetRead = Function("this.x = 5; return this.x;");
const sloppySetReturnThisType = Function(
  "this.x = 5; return Object.prototype.toString.call(this) + ':' + this.x;",
);

(Number.prototype as any).__perryTempSetRead = sloppySetRead;
(Number.prototype as any).__perryTempSetReturnThisType = sloppySetReturnThisType;
(String.prototype as any).__perryTempSetRead = sloppySetRead;
(String.prototype as any).__perryTempSetReturnThisType = sloppySetReturnThisType;
(Boolean.prototype as any).__perryTempSetRead = sloppySetRead;
(Boolean.prototype as any).__perryTempSetReturnThisType = sloppySetReturnThisType;

show("primitive method set/read", (5 as any).__perryTempSetRead());
show("primitive method this", (5 as any).__perryTempSetReturnThisType());
show("primitive x after", (5 as any).x);
show("string primitive method set/read", ("abc" as any).__perryTempSetRead());
show(
  "string primitive method this",
  ("abc" as any).__perryTempSetReturnThisType(),
);
show("string primitive x after", ("abc" as any).x);
show("boolean primitive method set/read", (true as any).__perryTempSetRead());
show(
  "boolean primitive method this",
  (true as any).__perryTempSetReturnThisType(),
);
show("boolean primitive x after", (true as any).x);

delete (Number.prototype as any).__perryTempSetRead;
delete (Number.prototype as any).__perryTempSetReturnThisType;
delete (String.prototype as any).__perryTempSetRead;
delete (String.prototype as any).__perryTempSetReturnThisType;
delete (Boolean.prototype as any).__perryTempSetRead;
delete (Boolean.prototype as any).__perryTempSetReturnThisType;

const numberValueOf = Object.prototype.valueOf.call(5);
show(
  "Object.valueOf.call number tag",
  Object.prototype.toString.call(numberValueOf),
);
show("Object.valueOf.call number value", numberValueOf.valueOf());
