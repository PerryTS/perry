import Beta, { betaHolder } from "./cycle_b.ts";
function Alpha() {
  return "alpha";
}
(Alpha as any).tag = "alpha-tag";
export const alphaHolder = { Alpha };
export function describeBeta() {
  return Beta() + ":" + (Beta as any).tag + ":" + String(Beta === betaHolder.Beta);
}
export default Alpha;
