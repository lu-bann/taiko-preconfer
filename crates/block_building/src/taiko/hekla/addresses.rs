use std::str::FromStr;

use alloy_primitives::Address;
use k256::ecdsa::SigningKey;

use crate::encode_util::hex_decode;

pub const TAIKO_ANCHOR_ADDRESS: &str = "0x1670090000000000000000000000000000010001";
pub const GOLDEN_TOUCH_ADDRESS: &str = "0x0000777735367b36bC9B61C50022d9D0700dB4Ec";
pub const GOLDEN_TOUCH_PRIVATE_KEY: &str =
    "0x92954368afd3caa1f3ce3ead0069c1af414054aefe1ef9aeacc1bf426222ce38";

pub fn get_golden_touch_address() -> Address {
    Address::from_str(GOLDEN_TOUCH_ADDRESS).unwrap()
}

pub fn get_taiko_anchor_address() -> Address {
    Address::from_str(TAIKO_ANCHOR_ADDRESS).unwrap()
}

pub fn get_golden_touch_signing_key() -> SigningKey {
    let pkey_bytes = hex_decode(GOLDEN_TOUCH_PRIVATE_KEY).unwrap();
    SigningKey::from_slice(&pkey_bytes).unwrap()
}
