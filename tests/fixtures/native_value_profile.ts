import {
  type u32,
  type u64,
  type pod,
  type PodView,
  NativeArena,
  sizeof,
  alignof,
  offsetof,
} from "perry/native";

type Header = pod<{
  flags: u32;
  sequence: u64;
}>;

const size = sizeof<Header>();
const alignment = alignof<Header>();
const sequenceOffset = offsetof<Header>("sequence");
const arena = NativeArena.alloc(size);
const headers: PodView<Header> = arena.podView(0, 1);

console.log(
  "size=" + size +
    ",align=" + alignment +
    ",sequence=" + sequenceOffset +
    ",length=" + headers.length,
);

arena.dispose();
