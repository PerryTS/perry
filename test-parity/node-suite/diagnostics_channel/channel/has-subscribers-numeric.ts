import dc from "node:diagnostics_channel";

// Regression guard: a large numeric argument used to be reinterpreted
// as a raw StringHeader pointer inside `decode_string_value`, which
// could segfault on a wild dereference. Just check that calls with
// non-string arguments don't crash the process — both Node and Perry
// either throw a TypeError or return false, both of which are
// acceptable; what matters is "no segfault".
function safe(label: string, fn: () => unknown): void {
  try {
    const v = fn();
    console.log(label + ": no-throw type=" + typeof v);
  } catch (err: any) {
    console.log(label + ": threw=" + err?.name);
  }
}

safe("hasSubscribers(1e9)", () => dc.hasSubscribers(1e9 as any));
safe("hasSubscribers(0)", () => dc.hasSubscribers(0 as any));
