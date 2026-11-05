import Alpha, { alphaHolder } from "./cycle_a.ts";
function Beta() {
  return "beta";
}
(Beta as any).tag = "beta-tag";
export const betaHolder = { Beta };
export function describeAlpha() {
  return Alpha() + ":" + (Alpha as any).tag + ":" + String(Alpha === alphaHolder.Alpha);
}
export default Beta;
