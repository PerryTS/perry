### Changed

- Remove dependencies no code uses: `perry-tls-session` from `perry-ext-http`, `anyhow` and `serde` from `perry-updater`, and `http-body-util`, `rustls-native-certs`, `sha3` 0.12 and `spki` from `perry-stdlib`, along with the feature entries that enabled them. Drop the unreferenced `http-body-util`, `futures-util` and `zip` workspace dependencies.
