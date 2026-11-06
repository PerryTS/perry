// A second importer of the same default exports, plus a barrel re-export.
import Point from "./ctor.ts";
import kinds from "./plain.ts";
export { default as PointViaBarrel } from "./ctor.ts";
export { default as kindsViaBarrel } from "./plain.ts";
export function pointSeenHere() {
  return Point;
}
export function kindsSeenHere() {
  return kinds;
}
