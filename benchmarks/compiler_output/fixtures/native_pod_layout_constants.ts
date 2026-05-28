type U32Cell = PerryPod<{ value: PerryU32 }>;
type Config = PerryPod<{ seed: PerryU32; limit: PerryU32 }>;
type Packet = PerryPod<{ tag: PerryU32; count: PerryU32; total: number }>;

function nativePodLayoutConstants(): number {
  const packetCount: number = 4;
  const configBytes: number = sizeof<Config>();
  const packetBytes: number = sizeof<Packet>();
  const wordBytes: number = sizeof<U32Cell>();
  const totalBytes: number = configBytes + packetCount * packetBytes;
  const wordCount: number = totalBytes / wordBytes;
  const arena = NativeArena.alloc(totalBytes);
  const words = arena.view(Uint32Array, 0, wordCount);
  const configView: PerryPodView<Config> = arena.podView(0, 1);
  const packetView: PerryPodView<Packet> = arena.podView(configBytes, packetCount);

  const seedIndex: number = offsetof<Config>("seed") / wordBytes;
  const limitIndex: number = offsetof<Config>("limit") / wordBytes;
  words[seedIndex] = 11;
  words[limitIndex] = packetCount;

  let checksum = words[seedIndex] + words[limitIndex];
  packet_loop:
  for (let i: number = 0; i < packetCount; i++) {
    const baseBytes: number = configBytes + i * packetBytes;
    const tagIndex: number = (baseBytes + offsetof<Packet>("tag")) / wordBytes;
    const countIndex: number = (baseBytes + offsetof<Packet>("count")) / wordBytes;
    words[tagIndex] = i + 3;
    words[countIndex] = packetCount - i;
    checksum = checksum + words[tagIndex] + words[countIndex];
  }

  const liveViews = 0;
  arena.dispose();
  return checksum + liveViews + alignof<Packet>() + alignof<Config>();
}

console.log("native_pod_layout_constants:" + nativePodLayoutConstants());
