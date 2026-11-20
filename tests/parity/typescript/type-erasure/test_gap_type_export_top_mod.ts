// WPB4d helper: a module whose export list mixes value exports with TS type-only exports.
export type Color = "white" | "black";
export interface Piece { readonly color: Color; readonly v: number }
export class Chess {
  v = 1;
  w = 2;
  color: Color = "white";
  piece(): Piece { return { color: this.color, v: this.v }; }
}
export function make(): Chess { return new Chess(); }
export const START = "start";
