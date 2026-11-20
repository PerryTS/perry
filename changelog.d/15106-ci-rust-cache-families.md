CI jobs restore warm Rust dependencies again. Every Rust job in the core
workflows goes through `.github/actions/perry-rust-cache`: each cache family
has one saving job, a failing job still saves, the key ignores the runner's
preinstalled stable toolchain, and no job saves an sccache tarball, which had
kept the 10 GB cache budget full.
