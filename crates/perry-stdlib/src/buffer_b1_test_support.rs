//! Output witnesses sabotage the producing conversion, before any read.
pub(crate) unsafe fn sabotage_output(family: &str, value: f64) {
    if std::env::var("PERRY_B1_OUTPUT_SABOTAGE").ok().as_deref() == Some(family) {
        perry_runtime::buffer::bytes::no_gc(|scope| {
            let bytes = perry_runtime::buffer::bytes::bytes_mut(value, scope).unwrap();
            bytes[0] ^= 1;
        });
    }
}
pub(crate) unsafe fn read(value: f64) -> Vec<u8> {
    perry_runtime::buffer::bytes::no_gc(|scope| {
        perry_runtime::buffer::bytes::bytes(value, scope)
            .unwrap()
            .to_vec()
    })
}

#[cfg(all(
    feature = "bundled-ethers",
    feature = "tls-runtime",
    feature = "compression-gzip"
))]
#[test]
fn producer_sabotages_turn_output_witnesses_red() {
    for (family, witness) in [
        (
            "ethers",
            "ethers::b1_output_tests::empty_keccak_bytes_match_the_known_digest",
        ),
        (
            "tls",
            "tls::b1_output_tests::ticket_keys_and_certificate_byte_conversion",
        ),
        (
            "zlib",
            "buffer_b1_zlib_tests::stream_chunk_conversion_and_sync_roundtrip",
        ),
    ] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", witness, "--nocapture"])
            .env("PERRY_B1_OUTPUT_SABOTAGE", family)
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&output.stdout).contains("running 1 test"));
        assert!(
            !output.status.success(),
            "{family} sabotage left its producer witness green"
        );
        eprintln!("B1 {family} output sabotage: RED ({})", output.status);
    }
}
