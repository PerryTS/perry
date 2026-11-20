#[cfg(feature = "apple-api")]
pub use crate::cli::apple_jwt::generate;
#[cfg(not(feature = "apple-api"))]
pub fn generate(_: &str, _: &str, _: &str) -> anyhow::Result<String> {
    anyhow::bail!("Apple API signing requires a Perry build with the apple-api feature")
}
