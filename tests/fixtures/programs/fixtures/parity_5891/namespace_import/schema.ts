export const alpha = { id: 1 };
export const beta = { id: 2 };
export const gammaEnum = ["x", "y"] as const;
export function helper() { return "H"; }
export type Alpha = typeof alpha;
export type Beta = typeof beta;
export interface Thing { id: number }
