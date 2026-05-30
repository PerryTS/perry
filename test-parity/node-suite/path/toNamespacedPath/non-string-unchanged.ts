import path from "node:path";

const obj = { marker: 1 };
const arr = ["x"];

function kind(value: any): string {
  if (value === null) return "null";
  if (Array.isArray(value)) return "array";
  return typeof value;
}

function check(label: string, value: any, result: any) {
  console.log(label, result === value, typeof result, kind(result));
}

check("default number", 123, path.toNamespacedPath(123 as any));
check("default undefined", undefined, path.toNamespacedPath(undefined as any));
check("posix null", null, path.posix.toNamespacedPath(null as any));
check("posix object", obj, path.posix.toNamespacedPath(obj as any));
check("win32 boolean", true, path.win32.toNamespacedPath(true as any));
check("win32 array", arr, path.win32.toNamespacedPath(arr as any));

const posix = path.posix;
const win32 = path.win32;
check("posix alias array", arr, posix.toNamespacedPath(arr as any));
check("win32 alias object", obj, win32.toNamespacedPath(obj as any));

console.log("posix string", JSON.stringify(path.posix.toNamespacedPath("/tmp/x")));
console.log("win32 string", JSON.stringify(path.win32.toNamespacedPath("C:\\x")));
