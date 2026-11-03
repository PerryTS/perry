use super::{
    bindings::wasi::random::{insecure, insecure_seed, random},
    State,
};

// Match the locked adapter's default maximum before allocating guest requests.
const MAX_RANDOM_BYTES: u64 = 64 << 20;

pub(super) fn fill(bytes: &mut [u8]) -> wasmtime::Result<()> {
    getrandom::fill(bytes).map_err(|error| wasmtime::format_err!("OS randomness: {error}"))
}

fn bytes(len: u64) -> wasmtime::Result<Vec<u8>> {
    wasmtime::ensure!(
        len <= MAX_RANDOM_BYTES,
        "requested random length exceeds 64 MiB"
    );
    let mut bytes = vec![0; usize::try_from(len)?];
    fill(&mut bytes)?;
    Ok(bytes)
}

fn random_u64() -> wasmtime::Result<u64> {
    let mut bytes = [0; 8];
    fill(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

impl random::Host for State {
    fn get_random_bytes(&mut self, len: u64) -> wasmtime::Result<Vec<u8>> {
        bytes(len)
    }
    fn get_random_u64(&mut self) -> wasmtime::Result<u64> {
        random_u64()
    }
}
impl insecure::Host for State {
    fn get_insecure_random_bytes(&mut self, len: u64) -> wasmtime::Result<Vec<u8>> {
        bytes(len)
    }
    fn get_insecure_random_u64(&mut self) -> wasmtime::Result<u64> {
        random_u64()
    }
}
impl insecure_seed::Host for State {
    fn insecure_seed(&mut self) -> wasmtime::Result<(u64, u64)> {
        Ok(self.insecure_seed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WasiOptions;

    #[test]
    fn random_lengths_limits_and_seed_contract() {
        let mut state = State::new(&WasiOptions::default()).unwrap();
        assert!(random::Host::get_random_bytes(&mut state, 0)
            .unwrap()
            .is_empty());
        assert_eq!(
            random::Host::get_random_bytes(&mut state, 257)
                .unwrap()
                .len(),
            257
        );
        assert_eq!(
            insecure::Host::get_insecure_random_bytes(&mut state, 33)
                .unwrap()
                .len(),
            33
        );
        assert!(random::Host::get_random_bytes(&mut state, MAX_RANDOM_BYTES + 1).is_err());
        assert!(insecure::Host::get_insecure_random_bytes(&mut state, u64::MAX).is_err());
        let seed = insecure_seed::Host::insecure_seed(&mut state).unwrap();
        let _ = insecure::Host::get_insecure_random_bytes(&mut state, 64).unwrap();
        assert_eq!(
            insecure_seed::Host::insecure_seed(&mut state).unwrap(),
            seed
        );
    }
}
