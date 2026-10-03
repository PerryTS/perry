// make is a hoisted function. This runs before producer's module body, and
// cannot rely on that body's initialization to mint its literal final shape.
import { make } from './producer.ts';
export const importerFirst = make(17).m();
