// Behavioral parity test for the `ethers` npm utility surface.
//
// `ethers` has no native binding: this compiles the real npm package (a root
// dependency) from source. The numeric/address helpers are pure functions of
// their inputs, so the output is byte-for-byte deterministic against Node.

import { formatEther, formatUnits, getAddress, parseEther, parseUnits } from "ethers";

// EIP-55 checksum.
console.log("getAddress lower:", getAddress("0x5aaeb6053f3e94c9b9a09f33669435e7ef1beaed"));
console.log("getAddress upper:", getAddress("0x5AAEB6053F3E94C9B9A09F33669435E7EF1BEAED"));

// parseEther / formatEther round-trip — 1.5 ETH → 1500000000000000000 wei.
const oneAndHalf = parseEther("1.5");
console.log("parseEther 1.5:", oneAndHalf.toString());
console.log("formatEther round-trip:", formatEther(oneAndHalf));

// parseUnits / formatUnits at common decimal positions.
const usdc = parseUnits("123.456789", 6);
console.log("parseUnits 6dec:", usdc.toString());
console.log("formatUnits 6dec:", formatUnits(usdc, 6));

const gwei = parseUnits("25", 9);
console.log("parseUnits gwei:", gwei.toString());
console.log("formatUnits gwei:", formatUnits(gwei, 9));

console.log("ethers parity: ok");
