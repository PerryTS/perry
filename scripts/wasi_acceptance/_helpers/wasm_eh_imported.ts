export const thrower = {
  get(carrier: any, key: string) {
    if (carrier[key] === undefined) throw new Error("imported " + key);
    return carrier[key];
  },
};
