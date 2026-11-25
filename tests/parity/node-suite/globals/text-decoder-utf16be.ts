const show = (label: string, bytes: number[], fatal = false) => {
  try {
    const decoder = new TextDecoder(label, { fatal: fatal });
    console.log(label, fatal, decoder.encoding, JSON.stringify(decoder.decode(new Uint8Array(bytes))));
  } catch (error) {
    console.log(label, fatal, (error as { code?: string }).code);
  }
};

const euro = [0x00, 0x41, 0x20, 0xac];
for (const label of ["utf-16le", "utf-16be", "unicodefffe", " UTF-16BE\n", "utf16be", "utf-16-be"]) {
  show(label, euro);
}

const inputs = [
  [],
  [0xd8, 0x3d, 0xde, 0x00, 0x00, 0x41], // surrogate pair
  [0xd8, 0x3d, 0x00, 0x41], // lone high surrogate
  [0xde, 0x00, 0x00, 0x41], // lone low surrogate
  [0x00, 0x41, 0x42], // odd trailing byte
  [0xd8, 0x3d, 0x41], // odd trailing byte after a high surrogate
  [0xd8, 0x3d], // high surrogate at the end
];
for (const bytes of inputs) {
  for (const fatal of [false, true]) {
    show("utf-16be", bytes, fatal);
    // The same code units in the other byte order.
    const swapped = bytes.slice();
    for (let i = 0; i + 1 < swapped.length; i += 2) {
      [swapped[i], swapped[i + 1]] = [swapped[i + 1], swapped[i]];
    }
    show("utf-16le", swapped, fatal);
  }
}
