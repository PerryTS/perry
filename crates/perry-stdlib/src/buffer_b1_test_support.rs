//! Output witnesses sabotage the producing conversion, before any read.
pub(crate) unsafe fn sabotage_output(family: &str, value: f64) {
    if std::env::var("PERRY_B1_OUTPUT_SABOTAGE").ok().as_deref() == Some(family) {
        let ptr = perry_runtime::JSValue::from_bits(value.to_bits())
            .as_pointer::<perry_runtime::buffer::BufferHeader>()
            .cast_mut();
        *perry_runtime::buffer::buffer_data_mut(ptr) ^= 1;
    }
}

pub(crate) unsafe fn read(value: f64) -> Vec<u8> {
    let ptr = perry_runtime::JSValue::from_bits(value.to_bits())
        .as_pointer::<perry_runtime::buffer::BufferHeader>();
    std::slice::from_raw_parts(
        perry_runtime::buffer::buffer_data(ptr),
        (*ptr).length as usize,
    )
    .to_vec()
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
            "zlib::b1_output_tests::stream_chunk_conversion_and_sync_roundtrip",
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
