// Exported functions, classes, constants for cross-module testing

export const PI = 3.14159265;
export const E = 2.71828182;

export function add(a: number, b: number): number {
  return a + b;
}

export function multiply(a: number, b: number): number {
  return a * b;
}

export class Vector {
  x: number;
  y: number;

  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }

  magnitude(): number {
    return Math.sqrt(this.x * this.x + this.y * this.y);
  }

  toString(): string {
    return "Vector(" + this.x + ", " + this.y + ")";
  }
}

export class Point extends Vector {
  label: string;

  constructor(x: number, y: number, label: string) {
    super(x, y);
    this.label = label;
  }

  toString(): string {
    return this.label + ": (" + this.x + ", " + this.y + ")";
  }
}

export let counter = 0;

export function incrementCounter(): number {
  counter++;
  return counter;
}
