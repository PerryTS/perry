function nativeOwnedPositive(): number {
  const n = 16;
  const arena = NativeArena.alloc(n * 8);
  const view = arena.view(Float64Array, 0, n);

  native_owned_positive_write:
  for (let i: number = 0; i < n; i++) {
    view[i] = i + 0.5;
  }

  let sum = 0;
  native_owned_positive_read:
  for (let i: number = 0; i < n; i++) {
    sum = sum + view[i];
  }

  arena.dispose();
  return sum;
}

function nativeOwnedKinds(): number {
  const arena = NativeArena.alloc(96);
  const u8 = arena.view(Uint8Array, 0, 8);
  const i16 = arena.view(Int16Array, 8, 4);
  const u32 = arena.view(Uint32Array, 16, 4);
  const f32 = arena.view(Float32Array, 32, 4);
  const f64 = arena.view(Float64Array, 48, 4);

  u8[0] = 7;
  i16[1] = -11;
  u32[2] = 1234;
  f32[3] = 2.5;
  f64[0] = 9.25;

  const sum = i16[1] + u32[2] + f32[3] + f64[0];
  arena.dispose();
  return sum;
}

function disposedFallback(): number {
  const arena = NativeArena.alloc(64);
  const view = arena.view(Float64Array, 0, 8);
  arena.dispose();
  try {
    return view[0];
  } catch (_err) {
    return 17;
  }
}

function staleLengthFallback(): number {
  const arena = NativeArena.alloc(64);
  let len = 8;
  const view = arena.view(Float64Array, 0, len);
  len = 4;
  const value = view[0];
  arena.dispose();
  return value;
}

function mutableAliasFallback(): number {
  const arena = NativeArena.alloc(64);
  const view = arena.view(Float64Array, 0, 8);
  const alias = view;
  alias[0] = 3.5;
  const value = alias[0];
  arena.dispose();
  return value;
}

function missingOwnerRootFallback(): number {
  let arena = NativeArena.alloc(64);
  const view = arena.view(Float64Array, 0, 8);
  arena = NativeArena.alloc(64);
  const value = view[0];
  arena.dispose();
  return value;
}

function escapeNativeView(_value: any): number {
  return 0;
}

function escapingUnownedPointerFallback(): number {
  const arena = NativeArena.alloc(64);
  const view = arena.view(Float64Array, 0, 8);
  escapeNativeView(view);
  const value = view[0];
  arena.dispose();
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
