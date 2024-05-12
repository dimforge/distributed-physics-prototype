use serde::{Deserialize, Serialize};

pub fn serialize(value: &impl Serialize) -> anyhow::Result<Vec<u8>> {
    Ok(lz4_flex::compress_prepend_size(&bincode::serialize(value)?))
}

pub fn deserialize<Out: for<'a> Deserialize<'a>>(value: &[u8]) -> anyhow::Result<Out> {
    Ok(bincode::deserialize(&lz4_flex::decompress_size_prepended(
        value,
    )?)?)
}
