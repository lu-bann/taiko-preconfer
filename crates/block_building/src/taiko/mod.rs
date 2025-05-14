mod anchor;
pub mod contracts;

use std::{str::FromStr, sync::LazyLock};

use alloy_primitives::{Address, ChainId};
pub use anchor::create_anchor_transaction;
use k256::ecdsa::SigningKey;

use crate::util::hex_decode;
pub mod sign;

pub static GOLDEN_TOUCH_SIGNING_KEY_HEKLA: LazyLock<SigningKey> = LazyLock::new(|| {
    let pkey_bytes =
        hex_decode("0x92954368afd3caa1f3ce3ead0069c1af414054aefe1ef9aeacc1bf426222ce38").unwrap();
    SigningKey::from_slice(&pkey_bytes).unwrap()
});

pub const CHAIN_ID_HEKLA: ChainId = 167009u64;

pub static GOLDEN_TOUCH_ADDRESS_HEKLA: LazyLock<Address> =
    LazyLock::new(|| Address::from_str("0x0000777735367b36bC9B61C50022d9D0700dB4Ec").unwrap());
pub static TAIKO_ANCHOR_ADDRESS_HEKLA: LazyLock<Address> =
    LazyLock::new(|| Address::from_str("0x1670090000000000000000000000000000010001").unwrap());
