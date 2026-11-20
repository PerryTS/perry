// Default and rest parameters behind `export default <identifier>`.
function withDefaults(a?: unknown, b: number = 5, ...rest: unknown[]) {
  return String(a) + "," + b + "," + rest.length;
}
export default withDefaults;
