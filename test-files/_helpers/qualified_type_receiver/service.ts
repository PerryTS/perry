export interface Service {
  readonly sep: string;
  readonly join: (...segments: ReadonlyArray<string>) => string;
  readonly find: (needle: string) => string;
}

export interface Box<T> {
  readonly join: (...segments: ReadonlyArray<T>) => string;
}
