import { native_memory_fixture_sum } from "@perry-fixtures/native-memory-fixture";

type Header = PerryPod<{
  seed: PerryU32;
  packetCount: PerryU32;
  packetStrideWords: PerryU32;
  scratchWords: PerryU32;
}>;
type Packet = PerryPod<{
  tag: PerryU32;
  count: PerryU32;
  checksum: PerryU32;
  reserved: PerryU32;
}>;
type U32Cell = PerryPod<{ value: PerryU32 }>;

function nativeMemoryFixture(): number {
  const packetCount: number = 8;
  const headerBytes: number = sizeof<Header>();
  const packetBytes: number = sizeof<Packet>();
  const wordBytes: number = sizeof<U32Cell>();
  const packetStrideWords: number = packetBytes / wordBytes;
  const packetWords: number = packetCount * packetStrideWords;
  const packetArenaBytes: number = packetCount * packetBytes;

  const headerArena = NativeArena.alloc(headerBytes);
  const packetArena = NativeArena.alloc(packetArenaBytes);
  const scratchArena = NativeArena.alloc(packetArenaBytes);
  const headerWords = headerArena.view(Uint32Array, 0, headerBytes / wordBytes);
  const packetWordsView = packetArena.view(Uint32Array, 0, packetWords);
  const scratchWords = scratchArena.view(Uint32Array, 0, packetWords);

  NativeMemory.fillU32(headerWords, 0);
  NativeMemory.fillU32(packetWordsView, 0);
  NativeMemory.fillU32(scratchWords, 0);

  const seedIndex: number = offsetof<Header>("seed") / wordBytes;
  const packetCountIndex: number = offsetof<Header>("packetCount") / wordBytes;
  const strideIndex: number = offsetof<Header>("packetStrideWords") / wordBytes;
  const scratchWordsIndex: number = offsetof<Header>("scratchWords") / wordBytes;
  headerWords[seedIndex] = 53;
  headerWords[packetCountIndex] = packetCount;
  headerWords[strideIndex] = packetStrideWords;
  headerWords[scratchWordsIndex] = packetWords;

  const tagOffsetWords: number = offsetof<Packet>("tag") / wordBytes;
  const countOffsetWords: number = offsetof<Packet>("count") / wordBytes;
  const checksumOffsetWords: number = offsetof<Packet>("checksum") / wordBytes;
  const reservedOffsetWords: number = offsetof<Packet>("reserved") / wordBytes;

  packet_loop:
  for (let i: number = 0; i < packetCount; i++) {
    const baseIndex: number = i * packetStrideWords;
    packetWordsView[baseIndex + tagOffsetWords] = i + 10;
    packetWordsView[baseIndex + countOffsetWords] = i + 1;
    packetWordsView[baseIndex + checksumOffsetWords] = 40 + i * 10;
    packetWordsView[baseIndex + reservedOffsetWords] = 7;
  }

  NativeMemory.copy(scratchWords, packetWordsView);

  const scratchPacketView: PerryPodView<Packet> = scratchArena.podView(0, packetCount);
  const nativeSum: number = native_memory_fixture_sum(scratchPacketView);

  let mirrorSum: number = 0;
  mirror_loop:
  for (let j: number = 0; j < packetWords; j++) {
    mirrorSum = mirrorSum + scratchWords[j];
  }

  let headerSum: number = 0;
  header_loop:
  for (let k: number = 0; k < headerBytes / wordBytes; k++) {
    headerSum = headerSum + headerWords[k];
  }

  scratchArena.dispose();
  packetArena.dispose();
  headerArena.dispose();
  return nativeSum + mirrorSum + headerSum + alignof<Packet>();
}

console.log("native_memory_fixture:" + nativeMemoryFixture());
