use std::str::FromStr;

use alloy_primitives::Address;
use k256::ecdsa::SigningKey;

use crate::encode_util::hex_decode;

pub const TAIKO_ANCHOR_ADDRESS: &str = "0x1670090000000000000000000000000000010001";
pub const TAIKO_INBOX_ADDRESS: &str = "0x79C9109b764609df928d16fC4a91e9081F7e87DB";
pub const TAIKO_WRAPPER_ADDRESS: &str = "0xD3f681bD6B49887A48cC9C9953720903967E9DC0";
pub const GOLDEN_TOUCH_ADDRESS: &str = "0x0000777735367b36bC9B61C50022d9D0700dB4Ec";
pub const GOLDEN_TOUCH_PRIVATE_KEY: &str =
    "0x92954368afd3caa1f3ce3ead0069c1af414054aefe1ef9aeacc1bf426222ce38";

pub fn get_golden_touch_address() -> Address {
    Address::from_str(GOLDEN_TOUCH_ADDRESS).unwrap()
}

pub fn get_taiko_anchor_address() -> Address {
    Address::from_str(TAIKO_ANCHOR_ADDRESS).unwrap()
}

pub fn get_taiko_inbox_address() -> Address {
    Address::from_str(TAIKO_INBOX_ADDRESS).unwrap()
}

pub fn get_taiko_wrapper_address() -> Address {
    Address::from_str(TAIKO_WRAPPER_ADDRESS).unwrap()
}

pub fn get_golden_touch_signing_key() -> SigningKey {
    let pkey_bytes = hex_decode(GOLDEN_TOUCH_PRIVATE_KEY).unwrap();
    SigningKey::from_slice(&pkey_bytes).unwrap()
}
