export interface Shape {
    sides: number;
}
export const name = "square";
export function make(): Shape {
    return { sides: 4 };
}
