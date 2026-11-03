export const getter = {
  get(carrier: any, key: string) { return carrier == null ? undefined : carrier[key]; },
};
