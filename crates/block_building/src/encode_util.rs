use hex::{FromHexError, decode, encode};

pub fn hex_encode<T: AsRef<[u8]>>(data: T) -> String {
    format!("0x{}", encode(data))
}

pub fn hex_decode(hex_string: &str) -> Result<Vec<u8>, FromHexError> {
    decode(hex_string.trim_start_matches("0x"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hex_encode() {
        let encoded = hex_encode([15u8]);
        assert_eq!(encoded, String::from("0x0f"));
    }

    #[test]
    fn test_hex_decode() {
        let hex_str = "0x10";
        let decoded = hex_decode(hex_str).unwrap();
        assert_eq!(decoded, vec![16u8]);
    }
}
