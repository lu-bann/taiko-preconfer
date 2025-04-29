use alloy_primitives::{Address, FixedBytes, Signature, SignatureError};

pub fn verify_signature(
    signature: &Signature,
    message_hash: &FixedBytes<32>,
    signer_address: &Address,
) -> Result<bool, SignatureError> {
    match signature.recover_address_from_prehash(message_hash) {
        Ok(recovered_address) => Ok(&recovered_address == signer_address),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::eip191_hash_message;
    use alloy_signer::SignerSync;
    use alloy_signer_local::PrivateKeySigner;

    use super::*;

    #[test]
    fn verification_passes_for_valid_signature() {
        let signer = PrivateKeySigner::random();
        let message = b"some msg";
        let signature = signer.sign_message_sync(message).unwrap();

        let message_hash = eip191_hash_message(message);
        let is_valid = verify_signature(&signature, &message_hash, &signer.address()).unwrap();
        assert!(is_valid);
    }

    #[test]
    fn verification_fails_for_wrong_signer() {
        let signer = PrivateKeySigner::random();
        let not_signer = PrivateKeySigner::random();
        let message = b"some msg";
        let signature = signer.sign_message_sync(message).unwrap();

        let message_hash = eip191_hash_message(message);
        let is_valid = verify_signature(&signature, &message_hash, &not_signer.address()).unwrap();
        assert!(!is_valid);
    }

    #[test]
    fn verification_fails_for_wrong_message_hash() {
        let signer = PrivateKeySigner::random();
        let message = b"some msg";
        let signature = signer.sign_message_sync(message).unwrap();

        let wrong_message = b"other msg";
        let wrong_message_hash = eip191_hash_message(wrong_message);
        let is_valid =
            verify_signature(&signature, &wrong_message_hash, &signer.address()).unwrap();
        assert!(!is_valid);
    }
}
