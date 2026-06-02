// process.stdout / process.stderr expose their file descriptors (1 / 2).
console.log("stdout.fd:", process.stdout.fd);
console.log("stderr.fd:", process.stderr.fd);
