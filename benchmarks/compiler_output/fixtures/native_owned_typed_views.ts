function nativeOwnedPositive(): number {
  const n = 16;
  const owner: any = __perry_native_arena_alloc(n * 8);
  const view = __perry_native_arena_view(owner, "Float64Array", 0, n) as Float64Array;

  native_owned_positive_write:
  for (let i: number = 0; i < n; i++) {
    view[i] = i + 0.5;
  }

  let sum = 0;
  native_owned_positive_read:
  for (let i: number = 0; i < n; i++) {
    sum = sum + view[i];
  }

  __perry_native_arena_dispose(owner);
  return sum;
}

function nativeOwnedKinds(): number {
  const owner: any = __perry_native_arena_alloc(96);
  const u8 = __perry_native_arena_view(owner, "Uint8Array", 0, 8) as Uint8Array;
  const i16 = __perry_native_arena_view(owner, "Int16Array", 8, 4) as Int16Array;
  const u32 = __perry_native_arena_view(owner, "Uint32Array", 16, 4) as Uint32Array;
  const f32 = __perry_native_arena_view(owner, "Float32Array", 32, 4) as Float32Array;
  const f64 = __perry_native_arena_view(owner, "Float64Array", 48, 4) as Float64Array;

  u8[0] = 7;
  i16[1] = -11;
  u32[2] = 1234;
  f32[3] = 2.5;
  f64[0] = 9.25;

  const sum = i16[1] + u32[2] + f32[3] + f64[0];
  __perry_native_arena_dispose(owner);
  return sum;
}

function disposedFallback(): number {
  const owner: any = __perry_native_arena_alloc(64);
  const view = __perry_native_arena_view(owner, "Float64Array", 0, 8) as Float64Array;
  __perry_native_arena_dispose(owner);
  try {
    return view[0];
  } catch (_err) {
    return 17;
  }
}

function staleLengthFallback(): number {
  const owner: any = __perry_native_arena_alloc(64);
  let len = 8;
  const view = __perry_native_arena_view(owner, "Float64Array", 0, len) as Float64Array;
  len = 4;
  const value = view[0];
  __perry_native_arena_dispose(owner);
  return value;
}

function mutableAliasFallback(): number {
  const owner: any = __perry_native_arena_alloc(64);
  const view = __perry_native_arena_view(owner, "Float64Array", 0, 8) as Float64Array;
  const alias = view;
  alias[0] = 3.5;
  const value = alias[0];
  __perry_native_arena_dispose(owner);
  return value;
}

function missingOwnerRootFallback(): number {
  let owner: any = __perry_native_arena_alloc(64);
  const view = __perry_native_arena_view(owner, "Float64Array", 0, 8) as Float64Array;
  owner = __perry_native_arena_alloc(64);
  const value = view[0];
  __perry_native_arena_dispose(owner);
  return value;
}

function escapeNativeView(_value: any): number {
  return 0;
}

function escapingUnownedPointerFallback(): number {
  const owner: any = __perry_native_arena_alloc(64);
  const view = __perry_native_arena_view(owner, "Float64Array", 0, 8) as Float64Array;
  escapeNativeView(view);
  const value = view[0];
  __perry_native_arena_dispose(owner);
  return value;
}

const checksum =
  nativeOwnedPositive() +
  nativeOwnedKinds() +
  disposedFallback() +
  staleLengthFallback() +
  mutableAliasFallback() +
  missingOwnerRootFallback() +
  escapingUnownedPointerFallback();

console.log("native_owned_typed_views:" + checksum);
