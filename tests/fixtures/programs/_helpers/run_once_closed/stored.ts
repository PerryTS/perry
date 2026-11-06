function make<T>(getter: () => T): () => T {
  return () => getter();
}
export const api: any = {};
api.make = make;
export const made = make(() => "stored");
