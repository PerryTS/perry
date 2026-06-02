const SIZE = 2048;

function typedArrayHazards(k: number): number {
  const aliasSource = new Uint16Array(4);
  const alias = aliasSource;
  let reassigned = new Int32Array(2);

  aliasSource[0] = 258;
  alias[k] = 513;
  reassigned = new Int32Array(2);
  reassigned[k] = -7;

  return alias[0] + alias[k] + reassigned[k];
}

const bytes = Buffer.alloc(SIZE + 8);
const probe = Buffer.alloc(32);

probe[0] = 0xff;
probe[1] = 0xfe;
probe[2] = 0x34;
probe[3] = 0x12;
probe[4] = 0xde;
probe[5] = 0xad;
probe[6] = 0xbe;
probe[7] = 0xef;
probe[8] = 0xfe;
probe[9] = 0xff;
probe[10] = 0xff;
probe[11] = 0xff;
probe[12] = 0x3f;
probe[13] = 0x80;
probe[14] = 0x00;
probe[15] = 0x00;
probe[24] = 0x00;
probe[25] = 0x00;
probe[26] = 0x00;
probe[27] = 0x00;
probe[28] = 0x00;
probe[29] = 0x00;
probe[30] = 0xf0;
probe[31] = 0x3f;

fill:
for (let i: number = 0; i < bytes.length; i++) {
  bytes[i] = i & 1;
}

let checksum: number = probe.readInt16BE(0);
checksum = checksum + probe.readUInt16LE(2);
checksum = checksum + probe.readUInt32BE(4);
checksum = checksum + probe.readInt32LE(8);
checksum = checksum + probe.readFloatBE(12) * 100;
checksum = checksum + probe.readDoubleLE(24) * 1000;

parse_u32_be:
for (let i: number = 0; i + 4 <= bytes.length; i++) {
  checksum = checksum + bytes.readUInt32BE(i);
}

parse_i32_le:
for (let i: number = 0; i + 4 <= bytes.length; i++) {
  checksum = checksum + bytes.readInt32LE(i);
}

parse_f32_be:
for (let i: number = 0; i + 4 <= bytes.length; i++) {
  checksum = checksum + bytes.readFloatBE(i);
}

parse_f64_le:
for (let i: number = 0; i + 8 <= bytes.length; i++) {
  checksum = checksum + bytes.readDoubleLE(i);
}

const u16 = new Uint16Array(256);
const i32s = new Int32Array(256);
const u32s = new Uint32Array(256);
const f32s = new Float32Array(256);
const f64s = new Float64Array(256);

typed_array_reads:
for (let i: number = 0; i < 256; i++) {
  checksum = checksum + u16[i];
  checksum = checksum + i32s[i];
  checksum = checksum + u32s[i];
  checksum = checksum + f32s[i];
  checksum = checksum + f64s[i];
}

checksum = checksum + typedArrayHazards(1);

console.log("width_aware_buffer_kernels:" + checksum);
