declare function gc(): void;

const SIZE = 4096;
const ROUNDS = 64;
const CHURN = 64;

const typedBuf = Buffer.alloc(SIZE);
const PACKET_TAG: number = 7;
const PACKET_GAIN: number = 1.5;
const PACKET_TOTAL: number = 2.25;
const PACKET_COUNT: number = SIZE;

seed_typed_packet:
for (let i = 0; i < typedBuf.length; i++) {
  typedBuf[i] = (i * 13) & 255;
}

function typedPacketKernel(
  buf: Buffer,
  tag: number,
  gain: number,
  total: number,
  count: number
): number {
  let checksum = 0;
  typed_packet_kernel:
  for (let i = 0; i < buf.length; i++) {
    const next = (buf[i] + tag + i) & 255;
    buf[i] = next;
    checksum = (checksum + next) | 0;
  }
  return checksum + count + gain * 4 + total;
}

function typedAllocationChurn(): number {
  let checksum = 0;
  typed_allocation_churn:
  for (let i = 0; i < CHURN; i++) {
    checksum = (checksum + (i & 255)) | 0;
  }
  gc();
  return checksum;
}

let typedTotal = 0;
typed_packet_rounds:
for (let r = 0; r < ROUNDS; r++) {
  typedTotal = (typedTotal + typedPacketKernel(
    typedBuf,
    PACKET_TAG,
    PACKET_GAIN,
    PACKET_TOTAL,
    PACKET_COUNT
  )) | 0;
}
typedTotal = (typedTotal + typedAllocationChurn()) | 0;

console.log("native_abi_packet_typed:" + typedTotal);
