// Issue #552 acceptance demo: location + photo-library picker + image compression + upload.
//
// Build:
//   cargo run --release -- examples/issue_552_demo.ts -o demo --target ios
//   cargo run --release -- examples/issue_552_demo.ts -o demo --target android
//
// Manifest entries required at app-bundle time (see types/perry/system/index.d.ts
// for full text):
//   iOS:     NSLocationWhenInUseUsageDescription in Info.plist
//   Android: ACCESS_FINE_LOCATION (and optionally ACCESS_COARSE_LOCATION) in
//            AndroidManifest.xml. ACTION_PICK_IMAGES (Photo Picker, API 33+)
//            and ext-sharp need no extra permissions.

import { App, VStack, Button, Text, setText, state } from "perry/ui"
import {
    geolocationGetCurrent,
    geolocationRequestPermission,
    imagePickerPick,
} from "perry/system"
import sharp from "sharp"
import * as fs from "fs"

const UPLOAD_URL = "https://example.com/upload"
const COMPRESSED_DIR = "/tmp"
const TARGET_BYTES = 500 * 1024

// Quality search: start at 80, halve until under TARGET_BYTES or quality ≤ 10.
function compressUnderLimit(srcPath: string, dstPath: string): number {
    let quality = 80
    let outBytes = Number.MAX_SAFE_INTEGER
    while (outBytes > TARGET_BYTES && quality >= 10) {
        sharp(srcPath).resize(1600, 1600).jpeg(quality).toFile(dstPath)
        const stat = fs.statSync(dstPath)
        outBytes = stat.size
        quality = Math.floor(quality / 2)
    }
    return outBytes
}

async function uploadOne(path: string, idx: number): Promise<void> {
    const dst = `${COMPRESSED_DIR}/issue552_${idx}.jpg`
    const finalBytes = compressUnderLimit(path, dst)
    setText("status", `compressed photo ${idx + 1}: ${finalBytes} bytes`)
    const body = fs.readFileSync(dst)
    const res = await fetch(UPLOAD_URL, {
        method: "POST",
        headers: { "Content-Type": "image/jpeg" },
        body,
    })
    if (!res.ok) {
        throw new Error(`upload ${idx} failed: HTTP ${res.status}`)
    }
}

function onLocate(): void {
    geolocationRequestPermission((status: string) => {
        if (status !== "granted") {
            setText("status", `location permission: ${status}`)
            return
        }
        geolocationGetCurrent(
            (lat: number, lng: number, accuracy: number, _ts: number) => {
                setText(
                    "status",
                    `location: ${lat.toFixed(5)}, ${lng.toFixed(5)} (±${accuracy}m)`,
                )
            },
            (err: string) => {
                setText("status", `location error: ${err}`)
            },
        )
    })
}

function onPickAndUpload(): void {
    imagePickerPick(2, true, (paths: string[]) => {
        if (paths.length === 0) {
            setText("status", "picker cancelled")
            return
        }
        setText("status", `picked ${paths.length} photo(s); compressing…`)
        ;(async () => {
            try {
                for (let i = 0; i < paths.length; i++) {
                    await uploadOne(paths[i], i)
                }
                setText("status", `uploaded ${paths.length} photo(s)`)
            } catch (e: any) {
                setText("status", `failed: ${e?.message ?? e}`)
            }
        })()
    })
}

App({
    title: "Issue #552 demo",
    body: VStack(16, [
        Text("Tap to test location + photo upload"),
        Text("(idle)", "status"),
        Button("Get current location", onLocate),
        Button("Pick 2 photos & upload", onPickAndUpload),
    ]),
})
