// The default export is module.exports, an ordinary object: it inherits the
// Object.prototype methods. safer-buffer (under iconv-lite) copies the module
// with exactly this loop (#13963).
import buffer from "node:buffer";

console.log("hasOwnProperty:", typeof buffer.hasOwnProperty);
console.log("own Buffer:", buffer.hasOwnProperty("Buffer"));
console.log("own missing:", buffer.hasOwnProperty("noSuchExport"));
console.log("enumerable Buffer:", buffer.propertyIsEnumerable("Buffer"));
console.log("isPrototypeOf:", buffer.isPrototypeOf({}));
console.log("inherited reads:", typeof buffer.toString, typeof buffer.valueOf);
console.log("constructor is Object:", buffer.constructor === Object);

const safer: Record<string, unknown> = {};
for (const key in buffer) {
  if (!buffer.hasOwnProperty(key)) continue;
  if (key === "SlowBuffer" || key === "Buffer") continue;
  safer[key] = (buffer as Record<string, unknown>)[key];
}
console.log("copied kMaxLength:", safer.kMaxLength === buffer.kMaxLength);
console.log("copied constants:", typeof safer.constants);
