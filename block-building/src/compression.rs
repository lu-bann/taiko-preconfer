use alloy_consensus::TxEnvelope;
use libdeflater::{CompressionError, CompressionLvl, Compressor};

fn compress_bytes(bytes: &[u8]) -> Result<Vec<u8>, CompressionError> {
    let mut comp = Compressor::new(CompressionLvl::best());
    let mut out = vec![0; comp.zlib_compress_bound(bytes.len())];
    let len = comp.zlib_compress(bytes, &mut out)?;
    out.truncate(len);
    Ok(out)
}

pub fn compress(txs: Vec<TxEnvelope>) -> Result<Vec<u8>, CompressionError> {
    let encoded = alloy_rlp::encode(txs);
    compress_bytes(&encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DATA_OFFSET: usize = 7;
    const LEN_IDX: usize = 3;

    #[test]
    fn test_compression_from_bytes() {
        let data: Vec<u8> = vec![18, 2, 3, 4];
        let out = compress_bytes(&data).unwrap();

        assert_eq!(out[LEN_IDX] as usize, data.len());
        assert_eq!(out[DATA_OFFSET..DATA_OFFSET + data.len()], data);
    }
}
