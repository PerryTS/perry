// A plain (non-constructor) declared function, default-exported by name.
function kinds(a?: unknown, b?: unknown, c?: unknown) {
  return typeof a + "," + typeof b + "," + typeof c;
}
(kinds as any).label = "kinds-label";
export const holder = { kinds };
export default kinds;
