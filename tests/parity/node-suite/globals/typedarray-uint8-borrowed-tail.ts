// @ts-nocheck

function describeValue(label, value) {
  if (value && typeof value === "object") {
    const isIterator = typeof value.next === "function" && typeof value[Symbol.iterator] === "function";
    const ctorName = isIterator ? "Iterator" : value.constructor ? value.constructor.name : "object";
    if (typeof value[Symbol.iterator] === "function") {
      const rendered = Array.from(value)
        .map((entry) => Array.isArray(entry) ? entry.join("=") : String(entry))
        .join(",");
      console.log(label + ":" + ctorName + ":" + rendered);
      return;
    }
    console.log(label + ":" + ctorName);
    return;
  }
  console.log(label + ":" + String(value));
}

function invoke(method, receiver, ...args) {
  return Int16Array.prototype[method].call(receiver, ...args);
}

function callback(value, index, array) {
  return value > 1 || (index === 0 && array.length === 3);
}

function reduceLog(accumulator, value, index, array) {
  return accumulator + value + "@" + index + "#" + array.constructor.name + ";";
}

function reduceNoInitial(accumulator, value, index, array) {
  return accumulator + "|" + value + "@" + index + "#" + array.constructor.name;
}

function inspectReceiver(label, receiver) {
  describeValue(label + " at", invoke("at", receiver, -1));
  describeValue(label + " at omitted", invoke("at", receiver));
  describeValue(label + " entries", invoke("entries", receiver));
  describeValue(label + " keys", invoke("keys", receiver));
  describeValue(label + " values", invoke("values", receiver));
  describeValue(label + " every", invoke("every", receiver, (value) => value > 0));
  describeValue(label + " some", invoke("some", receiver, (value) => value > 2));
  describeValue(label + " find", invoke("find", receiver, (value) => value > 1));
  describeValue(label + " forEach", (function () {
    const seen = [];
    const result = invoke("forEach", receiver, function (value, index, array) {
      seen.push(value + "@" + index + "#" + array.constructor.name);
    });
    return String(result) + ":" + seen.join(",");
  })());
  describeValue(label + " includes", invoke("includes", receiver, 2));
  describeValue(label + " includes from", invoke("includes", receiver, 2, -2));
  describeValue(label + " indexOf", invoke("indexOf", receiver, 2));
  describeValue(label + " indexOf from", invoke("indexOf", receiver, 2, -1));
  describeValue(label + " lastIndexOf", invoke("lastIndexOf", receiver, 2));
  describeValue(label + " lastIndexOf zero", invoke("lastIndexOf", receiver, 2, 0));
  describeValue(label + " join", invoke("join", receiver, "|"));
  describeValue(label + " toLocaleString", invoke("toLocaleString", receiver));
  describeValue(label + " reduce", invoke("reduce", receiver, reduceLog, ""));
  describeValue(label + " reduce no-init", invoke("reduce", receiver, reduceNoInitial));
  describeValue(label + " reduceRight", invoke("reduceRight", receiver, reduceLog, ""));
  describeValue(label + " reduceRight no-init", invoke("reduceRight", receiver, reduceNoInitial));
  describeValue(label + " with", invoke("with", receiver, 1, 9));
}

describeValue("lastIndexOf bind length", Int16Array.prototype.lastIndexOf.bind(new Uint8Array([1])).length);

inspectReceiver("uint8", new Uint8Array([1, 2, 3]));
inspectReceiver("buffer", Buffer.from([1, 2, 3]));
