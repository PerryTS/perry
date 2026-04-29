// demonstrates: per-API cryptography snippets shown in docs/src/stdlib/crypto.md
// docs: docs/src/stdlib/crypto.md
// platforms: macos, linux, windows
// run: false

// Each ANCHOR block below is the exact code that the crypto docs page renders
// inline (via {{#include ... :NAME}}). The whole file is compiled and linked
// by the doc-tests harness — `run: false` because bcrypt(cost=10) and argon2
// at default-tunings together push past the harness's 10 s soft timeout in
// CI. Compile + link is the contract here: it catches the API-shape drift
// these tests guard against (e.g. a `jwt.sign` overload changing).

// ANCHOR: bcrypt
import bcrypt from "bcrypt"

async function bcryptExample(): Promise<void> {
    const hash = await bcrypt.hash("mypassword", 10)
    const match = await bcrypt.compare("mypassword", hash)
    console.log(match) // true
}
// ANCHOR_END: bcrypt

// ANCHOR: argon2
import argon2 from "argon2"

async function argon2Example(): Promise<void> {
    const hash = await argon2.hash("mypassword")
    const valid = await argon2.verify(hash, "mypassword")
    console.log(valid) // true
}
// ANCHOR_END: argon2

// ANCHOR: jwt
import jwt from "jsonwebtoken"

function jwtExample(): void {
    const secret = "my-secret-key"

    // Sign a token
    const token = jwt.sign({ userId: 123, role: "admin" }, secret, {
        expiresIn: "1h",
    })

    // Verify a token
    const decoded: any = jwt.verify(token, secret)
    console.log(decoded.userId) // 123
}
// ANCHOR_END: jwt

// ANCHOR: crypto-node
import crypto from "crypto"

function cryptoExample(): void {
    // Hash
    const hash = crypto.createHash("sha256").update("data").digest("hex")

    // HMAC
    const hmac = crypto.createHmac("sha256", "secret").update("data").digest("hex")

    // Random bytes
    const bytes = crypto.randomBytes(32)

    console.log(`hash_len=${hash.length} hmac_len=${hmac.length} bytes_len=${bytes.length}`)
}
// ANCHOR_END: crypto-node

// ANCHOR: ethers
import { ethers } from "ethers"

function ethersExample(): void {
    // Utility functions
    const addr = ethers.getAddress("0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48")
    const wei = ethers.parseEther("1.5")
    const ether = ethers.formatEther(wei)
    console.log(`checksum: ${addr}`)
    console.log(`1.5 ether in wei → formatted back: ${ether}`)

    // Create a random wallet
    const wallet = ethers.Wallet.createRandom()
    console.log(`address: ${wallet.address}`)
    console.log(`privateKey length: ${wallet.privateKey.length}`)
}
// ANCHOR_END: ethers

// Reference everything so unused-import elimination doesn't strip the imports.
const _keep = [bcryptExample, argon2Example, jwtExample, cryptoExample, ethersExample]
console.log(`crypto-snippets: ${_keep.length}`)
