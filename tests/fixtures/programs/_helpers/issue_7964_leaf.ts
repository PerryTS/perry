export const NEVER = Object.freeze({ status: "aborted" });
export const $brand = Symbol("brand");

const _null = /^null$/i;
export { _null as null };

const _undefined = /^undefined$/i;
export { _undefined as undefined };

export function $constructor(): number {
  return 7;
}
