//! Focused worker_threads structured-clone regressions from the #6763 parity
//! umbrella. These assertions keep the already-fixed increments in the normal
//! integration suite instead of relying only on the full Node differential run.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn broadcast_and_port_clone_preserve_binary_values_and_errors() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
import {
  BroadcastChannel,
  markAsUncloneable,
  MessageChannel,
  receiveMessageOnPort,
} from "node:worker_threads";

function outcome(fn: () => void): string {
  try {
    fn();
    return "ok";
  } catch (error: any) {
    return `${error?.name} ${error?.code ?? ""}`;
  }
}

const sender = new BroadcastChannel("clone");
const receiver = new BroadcastChannel("clone");
const view = new Uint8Array([3, 1, 4]);
sender.postMessage(view);
view[0] = 9;
const broadcastPacket = receiveMessageOnPort(receiver);
const cloned = broadcastPacket?.message;
console.log(
  "typed",
  cloned instanceof Uint8Array,
  cloned.join(","),
  view.join(","),
);

const channel = new MessageChannel();
console.log(
  "broadcast-port",
  outcome(() => sender.postMessage({ port: channel.port1 })),
);
sender.close();
console.log("closed", outcome(() => sender.postMessage("late")));
receiver.close();

const root = { value: 1 };
markAsUncloneable(root);
console.log("marked-root", outcome(() => channel.port1.postMessage(root)));
const nested = { value: 2 };
markAsUncloneable(nested);
console.log(
  "marked-nested",
  outcome(() => channel.port1.postMessage({ nested })),
);

const buffer = new ArrayBuffer(4);
markAsUncloneable(buffer);
console.log("arraybuffer-post", outcome(() => channel.port1.postMessage(buffer)));
const portPacket = receiveMessageOnPort(channel.port2);
console.log(
  "arraybuffer-clone",
  portPacket?.message instanceof ArrayBuffer,
  portPacket?.message?.byteLength,
  buffer.byteLength,
);
channel.port1.close();
channel.port2.close();
"#,
    )
    .expect("write fixture");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .output()
        .expect("run compiled fixture");
    assert!(
        run.status.success(),
        "compiled fixture failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        concat!(
            "typed true 3,1,4 9,1,4\n",
            "broadcast-port DataCloneError 25\n",
            "closed InvalidStateError 11\n",
            "marked-root DataCloneError 25\n",
            "marked-nested DataCloneError 25\n",
            "arraybuffer-post ok\n",
            "arraybuffer-clone true 4 4\n",
        )
    );
}
