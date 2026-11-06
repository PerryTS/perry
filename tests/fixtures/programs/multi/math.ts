// Math utilities module that uses utils

import { square, double } from "./utils";

export function add(a: number, b: number): number {
    return a + b;
}

export function multiply(a: number, b: number): number {
    return a * b;
}

export function squareAndDouble(x: number): number {
    return double(square(x));
}
