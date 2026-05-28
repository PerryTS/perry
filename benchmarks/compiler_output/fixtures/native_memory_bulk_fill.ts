type U32Cell = PerryPod<{ value: PerryU32 }>;
type Config = PerryPod<{ seed: PerryU32; limit: PerryU32 }>;
type Packet = PerryPod<{
  tag: PerryU32;
  count: PerryU32;
  reserved0: PerryU32;
  reserved1: PerryU32;
  checksum: PerryU32;
}>;

function nativeMemoryBulkFill(): number {
  const packetCount: number = 8;
  const configBytes: number = sizeof<Config>();
  const packetBytes: number = sizeof<Packet>();
  const wordBytes: number = sizeof<U32Cell>();
  const configWords: number = configBytes / wordBytes;
  const packetStrideWords: number = packetBytes / wordBytes;
  const packetWords: number = packetCount * packetStrideWords;
  const packetArenaBytes: number = packetCount * packetBytes;
  const configArena = NativeArena.alloc(configBytes);
  const packetArena = NativeArena.alloc(packetArenaBytes);
  const configWordsView = configArena.view(Uint32Array, 0, configWords);
  const packetWordsView = packetArena.view(Uint32Array, 0, packetWords);
  const scratch = NativeArena.alloc(packetCount * packetBytes);
  const scratchWords = scratch.view(Uint32Array, 0, packetWords);

  NativeMemory.fillU32(packetWordsView, 0);
  NativeMemory.fillU32(scratchWords, 0);

  const seedIndex: number = offsetof<Config>("seed") / wordBytes;
  const limitIndex: number = offsetof<Config>("limit") / wordBytes;
  configWordsView[seedIndex] = 19;
  configWordsView[limitIndex] = packetCount;

  const tagOffsetWords: number = offsetof<Packet>("tag") / wordBytes;
  const countOffsetWords: number = offsetof<Packet>("count") / wordBytes;
  const checksumOffsetWords: number = offsetof<Packet>("checksum") / wordBytes;
  packet_loop:
  for (let i: number = 0; i < packetCount; i++) {
    const tag: number = i + 3;
    const count: number = packetCount - i;
    const baseIndex: number = i * packetStrideWords;
    packetWordsView[baseIndex + tagOffsetWords] = tag;
    packetWordsView[baseIndex + countOffsetWords] = count;
    packetWordsView[baseIndex + checksumOffsetWords] = (tag * count + 19) & 255;
  }

  NativeMemory.copy(scratchWords, packetWordsView);

  let checksum: number = configWordsView[seedIndex] + configWordsView[limitIndex];
  copy_loop:
  for (let j: number = 0; j < packetWords; j++) {
    checksum = checksum + scratchWords[j];
  }

  scratch.dispose();
  packetArena.dispose();
  configArena.dispose();
  return checksum + alignof<Packet>() + sizeof<Config>();
}

console.log("native_memory_bulk_fill:" + nativeMemoryBulkFill());
