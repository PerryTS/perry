export function cached<T>(getter: () => T): () => T {
  return () => getter();
}
export const flag = cached(() => true);
