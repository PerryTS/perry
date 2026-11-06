// Issue #3988: %TypedArray% static constructors, accessor brand checks,
// byte metadata, and iterator visibility.

function show(label: string, value: any) {
  console.log(label + ":", value);
}

function threwTypeError(fn: () => void) {
  try {
    fn();
    return false;
  } catch (err: any) {
    return err && err.name === "TypeError";
  }
}

const TypedArrayIntrinsic: any = Object.getPrototypeOf(Uint8Array);
const typedArrayProto: any = Object.getPrototypeOf(Uint8Array.prototype);

show("intrinsic from own", Object.prototype.hasOwnProperty.call(TypedArrayIntrinsic, "from"));
show("intrinsic of own", Object.prototype.hasOwnProperty.call(TypedArrayIntrinsic, "of"));
show("concrete from own", Object.prototype.hasOwnProperty.call(Uint8Array, "from"));
show("concrete from inherited", typeof (Uint8Array as any).from);
show("from desc", [
  typeof Object.getOwnPropertyDescriptor(TypedArrayIntrinsic, "from")!.value,
  Object.getOwnPropertyDescriptor(TypedArrayIntrinsic, "from")!.writable,
  Object.getOwnPropertyDescriptor(TypedArrayIntrinsic, "from")!.enumerable,
  Object.getOwnPropertyDescriptor(TypedArrayIntrinsic, "from")!.configurable,
].join(","));

const u8 = Uint8Array.from([257, 2], (value: number, index: number) => value + index);
show("Uint8Array.from mapped", Array.from(u8).join(","));

const i16 = Int16Array.of(1, -2, 65537);
show("Int16Array.of", Array.from(i16).join(","));

show("from null map throws", threwTypeError(() => Uint8Array.from([1], null as any)));
show("abstract call throws", threwTypeError(() => TypedArrayIntrinsic()));
show("concrete call throws", threwTypeError(() => (Uint8Array as any)()));

const u16 = Uint16Array.of(5, 6);
show("u16 byteLength", u16.byteLength);
show("u16 byteOffset", u16.byteOffset);
show("u16 buffer byteLength", u16.buffer.byteLength);

const big = new BigInt64Array(2);
show("big byteLength", big.byteLength);
show("big byteOffset", big.byteOffset);

const byteLengthGetter = Object.getOwnPropertyDescriptor(typedArrayProto, "byteLength")!.get!;
const bufferGetter = Object.getOwnPropertyDescriptor(typedArrayProto, "buffer")!.get!;
show("accessor invalid receiver", threwTypeError(() => byteLengthGetter.call({})));
show("buffer invalid receiver", threwTypeError(() => bufferGetter.call(undefined)));

show("iterator own", Object.prototype.hasOwnProperty.call(typedArrayProto, Symbol.iterator));
show("iterator alias", typedArrayProto[Symbol.iterator] === typedArrayProto.values);
show("iterator values", Array.from(u16[Symbol.iterator]()).join(","));
