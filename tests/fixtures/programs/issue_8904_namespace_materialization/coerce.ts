export interface CoercedNumber<T = unknown> {
  readonly input: T;
}

export type CoerceInput = string | number;

function _number(): (value: CoerceInput) => number {
  return (value: CoerceInput): number => Number(value);
}

export { _number as number };
