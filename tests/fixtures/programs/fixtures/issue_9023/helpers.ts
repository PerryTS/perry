import { Bag as ImportedBag } from "./bag.ts";
export function makeBag() { return new ImportedBag<number, number>(); }
export function getBag(store: Map<number, ImportedBag<number, number>>, key: number) {
 return store.get(key) ?? new ImportedBag<number, number>();
}
export function track(store: Map<number, ImportedBag<number, number>>, key: number, a: number, b: number) {
 if (!store.has(key)) store.set(key, new ImportedBag<number, number>());
 store.get(key)!.add(a, b);
}
