// fs.read accepts (fd, options, cb), (fd, buffer, cb), (fd, buffer, options, cb)
// and (fd, cb) as well as the positional six-argument form, and a bigint
// `position` is honoured. The spread call through `any` is the dynamic
// dispatch that Effect's NodeFileSystem `readAlloc` takes:
// `read(fd, { buffer, position: 0n }, cb)`. fs.readSync(fd, buffer) reads the
// whole buffer and takes a bigint position too.
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

const file = path.join(os.tmpdir(), `perry-fs-read-options-${process.pid}.txt`);
fs.writeFileSync(file, "hello world!");

function step(label: string, run: (cb: (err: any, n: number, buf: Buffer) => void) => void): Promise<void> {
    return new Promise((resolve) => {
        run((err, bytesRead, buf) => {
            console.log(label, err === null, bytesRead, buf.subarray(0, bytesRead).toString());
            resolve();
        });
    });
}

async function main() {
    const fd = fs.openSync(file, "r");

    await step("spread options bigint", (cb) => {
        const args: any[] = [fd, { buffer: Buffer.alloc(5), position: 6n }];
        (fs.read as any)(...args, cb);
    });
    await step("options number", (cb) => {
        fs.read(fd, { buffer: Buffer.alloc(5), position: 6 }, cb);
    });
    await step("buffer, options bigint", (cb) => {
        fs.read(fd, Buffer.alloc(5), { position: 6n }, cb);
    });
    await step("positional bigint", (cb) => {
        fs.read(fd, Buffer.alloc(5), 0, 5, 6n, cb);
    });
    await step("positional", (cb) => {
        fs.read(fd, Buffer.alloc(5), 0, 5, 6, cb);
    });
    await step("buffer", (cb) => {
        fs.read(fd, Buffer.alloc(5), cb);
    });
    await step("callback only", (cb) => {
        fs.read(fd, cb);
    });
    fs.closeSync(fd);

    const syncFd = fs.openSync(file, "r");
    const whole = Buffer.alloc(5);
    console.log("readSync whole", fs.readSync(syncFd, whole), whole.toString());
    const at = Buffer.alloc(5);
    console.log("readSync bigint", fs.readSync(syncFd, at, 0, 5, 6n), at.toString());
    fs.closeSync(syncFd);
    fs.unlinkSync(file);
}

main();
