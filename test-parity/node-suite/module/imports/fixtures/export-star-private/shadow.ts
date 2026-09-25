// Private `foo` must NOT be forwarded by `export *` (only exports are).
function foo(): string {
  return "fake";
}

export function useFoo(): string {
  return foo();
}
