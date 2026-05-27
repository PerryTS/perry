declare function gc(): void;

const SIZE = 4096;
const ROUNDS = 64;
const CHURN = 64;

const controlBuf = Buffer.alloc(SIZE);
const controlPacket: any = { tag: 7, gain: 1.5, total: 2.25, count: SIZE };
const controlBoxes: any[] = [];

seed_control_packet:
for (let i = 0; i < controlBuf.length; i++) {
  controlBuf[i] = (i * 13) & 255;
}

function controlPacketKernel(buf: Buffer, packet: any): number {
  let checksum = 0;
  control_packet_kernel:
  for (let i = 0; i < packet.count; i++) {
    const next = (buf[i] + packet.tag + i) & 255;
    buf[i] = next;
    checksum = (checksum + next) | 0;
  }
  return checksum + packet.count + packet.gain * 4 + packet.total;
}

function controlAllocationChurn(): number {
  let checksum = 0;
  control_allocation_churn:
  for (let i = 0; i < CHURN; i++) {
    const boxed = new Number(i + 0.5);
    const owner: any = __perry_native_arena_alloc(8);
    const view = __perry_native_arena_view(owner, "Uint8Array", 0, 8) as Uint8Array;
    view[0] = i & 255;
    checksum = (checksum + view[0]) | 0;
    controlBoxes.push(boxed);
    __perry_native_arena_dispose(owner);
  }
  gc();
  return checksum;
}

let controlTotal = 0;
control_packet_rounds:
for (let r = 0; r < ROUNDS; r++) {
  controlTotal = (controlTotal + controlPacketKernel(controlBuf, controlPacket)) | 0;
}
controlTotal = (controlTotal + controlAllocationChurn()) | 0;

console.log("native_abi_packet_control:" + controlTotal);
