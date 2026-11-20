import { importerFirst } from './consumer.ts';
export function make(value: number) { return { x: value, m: () => value }; }
export function other(value: number) { return { x: value, m: () => value + 100 }; }
export const seen = importerFirst;
