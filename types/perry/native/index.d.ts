// Type declarations for `perry/native` — explicit native layouts and owned
// arena storage. These public names map to Perry's existing verifier-backed
// native representation pipeline; the legacy ambient `Perry*` names remain
// available for compatibility.

/** Exact-width signed 32-bit integer when used in a native/POD contract. */
export type i32 = number & { readonly __perryI32?: never };

/** Exact-width signed 64-bit integer when used in a native/POD contract. */
export type i64 = number & { readonly __perryI64?: never };

/** Exact-width unsigned 32-bit integer when used in a native/POD contract. */
export type u32 = number & { readonly __perryU32?: never };

/** Exact-width unsigned 64-bit integer when used in a native/POD contract. */
export type u64 = number & { readonly __perryU64?: never };

/** Target pointer-sized unsigned integer in a native/POD contract. */
export type usize = number & { readonly __perryUSize?: never };

/** IEEE-754 binary32 value when used in a native/POD contract. */
export type f32 = number & { readonly __perryF32?: never };

/** IEEE-754 binary64 value when used in a native/POD contract. */
export type f64 = number & { readonly __perryF64?: never };

/**
 * A record with compiler-verified C field order, alignment, and padding.
 * Accepted fields are native scalar aliases, nested `pod` records, and the
 * compatible legacy `Perry*` scalar markers.
 */
export type pod<T> = T & { readonly __perryPod?: never };

/** A read-only indexed view of POD records stored in a `NativeArena`. */
export interface PodView<T extends pod<any>> {
  readonly length: number;
  readonly [index: number]: T;
  readonly __perryPodView?: never;
}

/** Compile-time size, in bytes, of a verified POD record. */
export declare function sizeof<T extends pod<any>>(): number;

/** Compile-time alignment, in bytes, of a verified POD record. */
export declare function alignof<T extends pod<any>>(): number;

/**
 * Compile-time byte offset of a field. Nested fields use a dotted path such
 * as `"header.flags"`; the path must be a string literal.
 */
export declare function offsetof<T extends pod<any>>(field: string): number;

/** Owned native allocation used to create typed-array and POD views. */
export interface NativeArena {
  view(kind: typeof Int8Array, byteOffset: number, length: number): Int8Array;
  view(kind: typeof Uint8Array, byteOffset: number, length: number): Uint8Array;
  view(kind: typeof Uint8ClampedArray, byteOffset: number, length: number): Uint8ClampedArray;
  view(kind: typeof Int16Array, byteOffset: number, length: number): Int16Array;
  view(kind: typeof Uint16Array, byteOffset: number, length: number): Uint16Array;
  view(kind: typeof Int32Array, byteOffset: number, length: number): Int32Array;
  view(kind: typeof Uint32Array, byteOffset: number, length: number): Uint32Array;
  view(kind: typeof Float32Array, byteOffset: number, length: number): Float32Array;
  view(kind: typeof Float64Array, byteOffset: number, length: number): Float64Array;
  view(kind: "Int8Array", byteOffset: number, length: number): Int8Array;
  view(kind: "Uint8Array", byteOffset: number, length: number): Uint8Array;
  view(kind: "Uint8ClampedArray", byteOffset: number, length: number): Uint8ClampedArray;
  view(kind: "Int16Array", byteOffset: number, length: number): Int16Array;
  view(kind: "Uint16Array", byteOffset: number, length: number): Uint16Array;
  view(kind: "Int32Array", byteOffset: number, length: number): Int32Array;
  view(kind: "Uint32Array", byteOffset: number, length: number): Uint32Array;
  view(kind: "Float32Array", byteOffset: number, length: number): Float32Array;
  view(kind: "Float64Array", byteOffset: number, length: number): Float64Array;
  podView<T extends pod<any>>(byteOffset: number, count: number): PodView<T>;
  dispose(): void;
}

export interface NativeArenaConstructor {
  alloc(byteLength: number): NativeArena;
}

export declare const NativeArena: NativeArenaConstructor;
