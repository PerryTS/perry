type Header = NativeRecord<{
  flags: Word;
  sequence: LongWord;
  gain: FloatWord;
}>;

type Tiny = NativeRecord<{
  kind: Octet;
  marker: Byte;
}>;

type Narrow = NativeRecord<{
  delta: SignedByte;
  count: HalfWord;
  offset: SignedHalfWord;
  pointerDelta: SignedSize;
}>;

type Nested = NativeRecord<{
  outer: Octet;
  inner: NativeRecord<{ code: HalfWord; delta: SignedByte }>;
}>;

const size = sizeOf<Header>();
const alignment = alignOf<Header>();
const sequenceOffset = offsetOf<Header>("sequence");
const arena = Arena.alloc(size);
const headers: PodView<Header> = arena.podView(0, 1);
const aliasedHeaders = headers;
const convertedFlags = Word(4_294_967_295);
const convertedSequence = LongWord(9_007_199_254_740_991);
const convertedGain = FloatWord(0.1);
const tinySize = sizeOf<Tiny>();
const markerOffset = offsetOf<Tiny>("marker");
const convertedOctet = Octet(255);
const tiny: Tiny = { kind: Octet(255), marker: 7 };
const narrowSize = sizeOf<Narrow>();
const narrowCountOffset = offsetOf<Narrow>("count");
const narrowOffsetOffset = offsetOf<Narrow>("offset");
const narrowPointerOffset = offsetOf<Narrow>("pointerDelta");
const convertedSignedByte = SignedByte(-128);
const convertedHalfWord = HalfWord(65_535);
const convertedSignedHalfWord = SignedHalfWord(-32_768);
const convertedSignedSize = SignedSize(-9_007_199_254_740_991);
const narrow: Narrow = {
  delta: SignedByte(-5),
  count: HalfWord(65_535),
  offset: SignedHalfWord(-1_024),
  pointerDelta: SignedSize(-42),
};
const convertedHeader: Header = {
  flags: Word(7),
  sequence: LongWord(42),
  gain: FloatWord(0.1),
};
const originalNested: Nested = {
  outer: Octet(7),
  inner: { code: HalfWord(513), delta: SignedByte(-8) },
};
let copiedNested = originalNested;
copiedNested.outer = Octet(9);
function mutateHeader(value: Header): void {
  value.flags = Word(99);
}
mutateHeader(convertedHeader);
let rejectedFraction = false;
let rejectedType = false;
let rejectedOctet = false;
let rejectedSignedByte = false;
let rejectedHalfWord = false;
let rejectedSignedHalfWord = false;
let rejectedSignedSize = false;
try {
  Word(1.5);
} catch {
  rejectedFraction = true;
}
try {
  Word("1" as any);
} catch {
  rejectedType = true;
}
try {
  Octet(256);
} catch {
  rejectedOctet = true;
}
try {
  SignedByte(-129);
} catch {
  rejectedSignedByte = true;
}
try {
  HalfWord(65_536);
} catch {
  rejectedHalfWord = true;
}
try {
  SignedHalfWord(32_768);
} catch {
  rejectedSignedHalfWord = true;
}
try {
  SignedSize(9_007_199_254_740_992);
} catch {
  rejectedSignedSize = true;
}

// Static imports are hoisted, including aliases used above their declaration.
import {
  i8 as SignedByte,
  i16 as SignedHalfWord,
  u8 as Octet,
  u16 as HalfWord,
  u32 as Word,
  u64 as LongWord,
  f32 as FloatWord,
  isize as SignedSize,
  type pod as NativeRecord,
  type byte as Byte,
  type PodView,
  NativeArena as Arena,
  sizeof as sizeOf,
  alignof as alignOf,
  offsetof as offsetOf,
} from "perry/native";

console.log(
  "size=" + size +
    ",align=" + alignment +
    ",sequence=" + sequenceOffset +
    ",length=" + aliasedHeaders.length +
    ",flags=" + convertedFlags +
    ",sequenceValue=" + convertedSequence +
    ",gainRounded=" + (convertedGain > 0.1) +
    ",tiny=" + tinySize + ":" + markerOffset + ":" + convertedOctet +
    ":" + tiny.kind + ":" + tiny.marker +
    ",narrow=" + narrowSize + ":" + narrowCountOffset + ":" + narrowOffsetOffset +
    ":" + narrowPointerOffset + ":" + convertedSignedByte + ":" + convertedHalfWord +
    ":" + convertedSignedHalfWord + ":" + convertedSignedSize + ":" + narrow.delta +
    ":" + narrow.count + ":" + narrow.offset + ":" + narrow.pointerDelta +
    ",header=" + convertedHeader.flags + ":" + convertedHeader.sequence + ":" + (convertedHeader.gain > 0.1) +
    ",podCopy=" + originalNested.outer + ":" + copiedNested.outer +
    ":" + originalNested.inner.code + ":" + copiedNested.inner.delta +
    ",rejectedFraction=" + rejectedFraction +
    ",rejectedType=" + rejectedType +
    ",rejectedOctet=" + rejectedOctet +
    ",rejectedSignedByte=" + rejectedSignedByte +
    ",rejectedHalfWord=" + rejectedHalfWord +
    ",rejectedSignedHalfWord=" + rejectedSignedHalfWord +
    ",rejectedSignedSize=" + rejectedSignedSize,
);

arena.dispose();
