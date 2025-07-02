use alloy_consensus::BlobTransactionSidecar;
use alloy_primitives::Bytes;

use c_kzg::{BYTES_PER_BLOB, Blob, Error as KzgError, KzgSettings};
use thiserror::Error;
use tracing::info;

use crate::util::now_as_millis;

const DATA_LENGTH_SIZE: usize = 4;
const FIELD_ELEMENTS_PER_ITERATION: usize = 4;
const DATA_SIZE_PER_ITERATION: usize = FIELD_ELEMENTS_PER_ITERATION * 31 + 3;
const ITERATIONS: usize = 1024;
pub const MAX_BLOB_DATA_SIZE: usize = DATA_SIZE_PER_ITERATION * ITERATIONS - DATA_LENGTH_SIZE;
const ENCODING_VERSION: u8 = 0;

#[derive(Error, Debug)]
pub enum BlobEncodeError {
    #[error("Blob size of {size} bytes exceeds {MAX_BLOB_DATA_SIZE} failed.")]
    BlobSize { size: usize },

    #[error("Invalid byte offset {offset}%32 != {mod_expected}.")]
    InvalidByteOffset { offset: usize, mod_expected: usize },

    #[error("Invalid byte encoding {byte} & {mask} != 0.")]
    InvalidByteEncoding { byte: u8, mask: u8 },

    #[error("Failed fitting data into blob. Processed {processed_bytes} of {total_bytes}.")]
    FailedFittingData {
        processed_bytes: usize,
        total_bytes: usize,
    },

    #[error("{0}")]
    Kzg(#[from] KzgError),
}

pub fn create_blob(data: &[u8]) -> Result<Blob, BlobEncodeError> {
    let mut blob_bytes = [0; BYTES_PER_BLOB];
    let mut read_offset = 0;
    let mut write_offset = 0;
    let mut buf31 = [0u8; 31];

    let next_idx = write_version_and_data_size(data.len() as u32, &mut buf31);

    let n = std::cmp::min(buf31.len() - next_idx, data.len());
    buf31[next_idx..next_idx + n].copy_from_slice(&data[read_offset..read_offset + n]);
    buf31[next_idx + n..].fill(0);
    read_offset += n;

    let x = read_byte(data, &mut read_offset);
    let a = x & MASK_1;
    write(&mut blob_bytes, &mut write_offset, a, &buf31)?;

    read_31_bytes(data, &mut read_offset, &mut buf31);
    let y = read_byte(data, &mut read_offset);
    let b = (y & MASK_2) | ((x & MASK_3) >> 2);
    write(&mut blob_bytes, &mut write_offset, b, &buf31)?;

    read_31_bytes(data, &mut read_offset, &mut buf31);
    let z = read_byte(data, &mut read_offset);
    let c = z & MASK_1;
    write(&mut blob_bytes, &mut write_offset, c, &buf31)?;

    read_31_bytes(data, &mut read_offset, &mut buf31);
    let d = ((z & MASK_3) >> 2) | ((y & MASK_4) >> 4);
    write(&mut blob_bytes, &mut write_offset, d, &buf31)?;

    let iterations = std::cmp::min(ITERATIONS, data.len() / 32 + 1);
    tracing::info!("iter");

    let result: Result<Vec<()>, BlobEncodeError> = (1..iterations)
        .map(|_| {
            read_31_bytes(data, &mut read_offset, &mut buf31);

            let x = read_byte(data, &mut read_offset);
            let a = x & MASK_1;
            write(&mut blob_bytes, &mut write_offset, a, &buf31)?;

            read_31_bytes(data, &mut read_offset, &mut buf31);
            let y = read_byte(data, &mut read_offset);
            let b = (y & MASK_2) | ((x & MASK_3) >> 2);
            write(&mut blob_bytes, &mut write_offset, b, &buf31)?;

            read_31_bytes(data, &mut read_offset, &mut buf31);
            let z = read_byte(data, &mut read_offset);
            let c = z & MASK_1;
            write(&mut blob_bytes, &mut write_offset, c, &buf31)?;

            read_31_bytes(data, &mut read_offset, &mut buf31);
            let d = ((z & MASK_3) >> 2) | ((y & MASK_4) >> 4);
            write(&mut blob_bytes, &mut write_offset, d, &buf31)?;
            Ok(())
        })
        .collect();
    result?;

    assert_all_bytes_processed(read_offset, data.len())?;

    Ok(c_kzg::Blob::from(blob_bytes))
}

pub fn tx_bytes_to_blobs(tx_bytes: Bytes) -> Result<Vec<Blob>, BlobEncodeError> {
    let blobs: Result<_, _> = tx_bytes
        .chunks(MAX_BLOB_DATA_SIZE)
        .map(create_blob)
        .collect();
    blobs
}

pub fn tx_bytes_to_sidecar(
    tx_bytes: Bytes,
    kzg_settings: &KzgSettings,
) -> Result<BlobTransactionSidecar, BlobEncodeError> {
    let blobs = tx_bytes_to_blobs(tx_bytes)?;
    tracing::info!("blobs to sidecar");
    blobs_to_sidecar(blobs, kzg_settings)
}

pub fn blobs_to_sidecar(
    blobs: Vec<c_kzg::Blob>,
    kzg_settings: &KzgSettings,
) -> Result<BlobTransactionSidecar, BlobEncodeError> {
    info!("blobs to sidecar start");

    let mut commitments = Vec::with_capacity(blobs.len());
    let mut proofs = Vec::with_capacity(blobs.len());

    let start = now_as_millis();
    for blob in blobs.iter() {
        let commitment = kzg_settings.blob_to_kzg_commitment(blob)?.to_bytes();
        let proof = kzg_settings
            .compute_blob_kzg_proof(blob, &commitment)?
            .to_bytes();
        commitments.push(commitment);
        proofs.push(proof);
    }
    let end = now_as_millis();
    info!("sidecar elapsed {} ms", end - start);

    Ok(BlobTransactionSidecar::from_kzg(blobs, commitments, proofs))
}

fn assert_byte_offset(offset: usize, mod_expected: usize) -> Result<(), BlobEncodeError> {
    if offset % 32 != mod_expected {
        return Err(BlobEncodeError::InvalidByteOffset {
            offset,
            mod_expected,
        });
    }
    Ok(())
}

fn assert_byte_encoding(byte: u8, mask: u8) -> Result<(), BlobEncodeError> {
    if byte & mask != 0 {
        return Err(BlobEncodeError::InvalidByteEncoding { byte, mask });
    }
    Ok(())
}

fn assert_all_bytes_processed(
    processed_bytes: usize,
    total_bytes: usize,
) -> Result<(), BlobEncodeError> {
    if processed_bytes != total_bytes {
        return Err(BlobEncodeError::FailedFittingData {
            processed_bytes,
            total_bytes,
        });
    }
    Ok(())
}

fn read_byte(data: &[u8], read_offset: &mut usize) -> u8 {
    if *read_offset >= data.len() {
        0
    } else {
        let out = data[*read_offset];
        *read_offset += 1;
        out
    }
}

fn read_31_bytes(data: &[u8], read_offset: &mut usize, buf31: &mut [u8; 31]) {
    if *read_offset >= data.len() {
        buf31.fill(0);
    } else {
        let remaining = data.len() - *read_offset;
        let n = std::cmp::min(31, remaining);
        buf31[..n].copy_from_slice(&data[*read_offset..*read_offset + n]);
        buf31[n..].fill(0);
        *read_offset += n;
    }
}

fn write_1(
    blob_bytes: &mut [u8; BYTES_PER_BLOB],
    write_offset: &mut usize,
    v: u8,
) -> Result<(), BlobEncodeError> {
    assert_byte_offset(*write_offset, 0)?;
    assert_byte_encoding(v, MASK_3)?;
    blob_bytes[*write_offset] = v;
    *write_offset += 1;
    Ok(())
}

fn write_31(
    blob_bytes: &mut [u8; BYTES_PER_BLOB],
    write_offset: &mut usize,
    buf31: &[u8; 31],
) -> Result<(), BlobEncodeError> {
    assert_byte_offset(*write_offset, 1)?;
    blob_bytes[*write_offset..*write_offset + 31].copy_from_slice(buf31);
    *write_offset += 31;
    Ok(())
}

fn write(
    blob_bytes: &mut [u8; BYTES_PER_BLOB],
    write_offset: &mut usize,
    v: u8,
    buf31: &[u8; 31],
) -> Result<(), BlobEncodeError> {
    write_1(blob_bytes, write_offset, v)?;
    write_31(blob_bytes, write_offset, buf31)
}

fn write_version_and_data_size(size: u32, buf31: &mut [u8; 31]) -> usize {
    buf31[0] = ENCODING_VERSION;
    buf31[1] = (size >> 16) as u8;
    buf31[2] = (size >> 8) as u8;
    buf31[3] = size as u8;
    4
}

const MASK_1: u8 = 0b0011_1111;
const MASK_2: u8 = 0b0000_1111;
const MASK_3: u8 = 0b1100_0000;
const MASK_4: u8 = 0b1111_0000;

#[cfg(test)]
mod tests {
    use alloy_eips::eip4844::env_settings::EnvKzgSettings;
    use alloy_primitives::{FixedBytes, keccak256};

    use super::*;

    fn test_data() -> Vec<u8> {
        vec![
            0xBE, 0x68, 0xB2, 0x32, 0x82, 0xC8, 0xEC, 0x40, 0x4B, 0x0F, 0xF8, 0x77, 0x33, 0x02,
            0xA0, 0x02, 0x02, 0xE1, 0xE9, 0xAA, 0x74, 0xEA, 0x50, 0x22, 0xD0, 0xAE, 0x47, 0x74,
            0x1F, 0x4B, 0x4A, 0x73, 0xA2, 0x12, 0x16, 0x37, 0x07, 0x01, 0x1F, 0x24, 0x64, 0x56,
            0xAD, 0x41, 0x5F, 0x65, 0x58, 0xB0, 0x82, 0x24, 0x49, 0x25, 0xD9, 0x8F, 0x3D, 0x17,
            0x63, 0x0D, 0x94, 0x89, 0xF7, 0xEB, 0xA2, 0xFC, 0xF6, 0x7D, 0x35, 0x3B, 0xCF, 0xAA,
            0x72, 0x07, 0xA5, 0x18, 0x00, 0xAF, 0xD3, 0x2D, 0x70, 0x2B, 0x92, 0xE1, 0xD8, 0x14,
            0xB8, 0x09, 0xEB, 0x05, 0x05, 0x4E, 0x9D, 0x8A, 0x39, 0x4B, 0xD1, 0x7C, 0x5E, 0xB0,
            0x60, 0xE7, 0xD8, 0x53, 0x1A, 0xE3, 0xDB, 0x02, 0xD6, 0xE4, 0xCC, 0x04, 0xA1, 0xF5,
            0x31, 0x14, 0x79, 0x92, 0xC1, 0x5E, 0x82, 0x42, 0x14, 0x1C, 0x19, 0xAB, 0x89, 0xAA,
            0xC1, 0x1F, 0x3A, 0x6E, 0x5D, 0x15, 0xD1, 0x80, 0x50, 0xE3, 0x60, 0x49, 0xF0, 0x3F,
            0x67, 0xE4, 0x1C, 0x98, 0x54, 0xDC, 0xDA, 0x48, 0xCC, 0x82, 0x6A, 0xD6, 0x64, 0x94,
            0xF3, 0x64, 0x52, 0xD9, 0x25, 0xAD, 0xC5, 0xEB, 0xD3, 0x8E, 0xB3, 0xCD, 0xF4, 0x69,
            0xDF, 0xFC, 0xB7, 0xE4, 0x44, 0xAC, 0xDB, 0x92, 0xDE, 0xF0, 0x6C, 0xAA, 0xDD, 0x12,
            0xF4, 0x6B, 0x3F, 0x62, 0x45, 0xC3, 0xB5, 0x19, 0xEB, 0x32, 0x3E, 0xAD, 0x7B, 0x9D,
            0x61, 0x58, 0xF1, 0x1D,
        ]
    }

    #[test]
    fn test_blob_sidecar_valid_proof_creation() {
        let data = test_data();
        let blob = create_blob(&data).unwrap();
        let settings = EnvKzgSettings::Default.get();
        let sidecar = blobs_to_sidecar(vec![blob], settings).unwrap();
        let sidecar_item = sidecar.into_iter().next().unwrap();
        assert!(sidecar_item.verify_blob_kzg_proof().is_ok());
    }

    #[test]
    fn test_encode_blob() {
        let data = test_data();

        let blob: Blob = create_blob(&data).unwrap();

        assert_eq!(
            keccak256(&blob),
            FixedBytes::<32>::from_slice(
                &hex::decode("f7ae80fe2d0ea322c04bc51e4a89495329f0628fbabacfcf6cc76dba3317bef8")
                    .unwrap()
            )
        );
    }
}
