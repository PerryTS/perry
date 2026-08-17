type Header = NativeRecord<{
  flags: Word;
  sequence: LongWord;
  gain: FloatWord;
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
let rejectedFraction = false;
let rejectedType = false;
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

// Static imports are hoisted, including aliases used above their declaration.
import {
  u32 as Word,
  u64 as LongWord,
  f32 as FloatWord,
  type pod as NativeRecord,
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
    ",rejectedFraction=" + rejectedFraction +
    ",rejectedType=" + rejectedType,
);

arena.dispose();
