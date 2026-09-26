Repair the Next App Route release fixture’s separate provider workspace by
removing the deleted `bundled-decimal` feature and regenerating its lockfile
from the current root workspace pins. The provider graph no longer includes
Tokio-family dependencies (#11404). `cargo metadata --locked --offline`
resolves both provider crates successfully; all external package pins match
the root lockfile. No workspace version bump.
