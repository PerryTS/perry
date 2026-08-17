type Header = NativeRecord<{
  flags: Word;
  sequence: LongWord;
  gain: FloatWord;
}>;

type Tiny = NativeRecord<{
  kind: Octet;
  marker: Byte;
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
const convertedHeader: Header = {
  flags: Word(7),
  sequence: LongWord(42),
  gain: FloatWord(0.1),
};
let rejectedFraction = false;
let rejectedType = false;
let rejectedOctet = false;
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

// Static imports are hoisted, including aliases used above their declaration.
import {
  u8 as Octet,
  u32 as Word,
  u64 as LongWord,
  f32 as FloatWord,
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
    ",header=" + convertedHeader.flags + ":" + convertedHeader.sequence + ":" + (convertedHeader.gain > 0.1) +
    ",rejectedFraction=" + rejectedFraction +
    ",rejectedType=" + rejectedType +
    ",rejectedOctet=" + rejectedOctet,
);

arena.dispose();
