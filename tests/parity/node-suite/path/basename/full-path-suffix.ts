import path from "node:path";

for (const [label, actual] of [
  ["posix full suffix", path.basename("/foo", "/foo")],
  ["posix basename suffix", path.basename("/foo", "foo")],
  ["posix nested full suffix", path.basename("/foo/bar", "/foo/bar")],
  ["posix plain full suffix", path.basename("foo", "foo")],
  ["posix undefined suffix", path.basename("/foo", undefined as any)],
  ["win32 full suffix", path.win32.basename("C:\\foo", "C:\\foo")],
  ["win32 basename suffix", path.win32.basename("C:\\foo", "foo")],
  ["win32 undefined suffix", path.win32.basename("C:\\foo", undefined as any)],
] as const) {
  console.log(label + ":", JSON.stringify(actual));
}
