A Worker whose script was not compiled into the binary (a filename built at
run time that names no compiled entry, or an eval Worker) now constructs and
fails the way a script that cannot load fails in Node and in a Web Worker: an
asynchronous `error` event whose message names the cause, followed by exit
code 1 (the worker throws `ERR_WORKER_NOT_COMPILED` /
`ERR_WORKER_EVAL_NOT_COMPILED`; like every worker error, only name, message,
stack and cause cross to the parent). It threw from the
constructor, which no Node or browser code expects; opentui's tree-sitter
client constructs its worker from a `{ type: "file" }` import path, so
opencode's TUI crashed rendering the first assistant reply. A malformed
filename still throws `ERR_WORKER_PATH` synchronously, and a `$perryfs/...`
path (the binary's own virtual file system) counts as absolute.
