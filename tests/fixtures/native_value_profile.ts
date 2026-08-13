type Header = NativeRecord<{
  flags: Word;
  sequence: LongWord;
}>;

const size = sizeOf<Header>();
const alignment = alignOf<Header>();
const sequenceOffset = offsetOf<Header>("sequence");
const arena = Arena.alloc(size);
const headers: PodView<Header> = arena.podView(0, 1);
const aliasedHeaders = headers;

// Static imports are hoisted, including aliases used above their declaration.
import {
  type u32 as Word,
  type u64 as LongWord,
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
    ",length=" + aliasedHeaders.length,
);

arena.dispose();
