use std::fmt::{Debug, Formatter, Result as FmtResult};

pub struct Secret(String);

impl Secret {
    pub fn read(&self) -> String {
        self.0.clone()
    }
}

impl Debug for Secret {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        write!(f, "Secret(********)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_does_not_get_exposed_in_debug_info() {
        let secret_str = "password".to_string();
        let secret = Secret(secret_str.clone());
        let secret_dbg = format!("{secret:?}");
        assert_eq!(secret_dbg, "Secret(********)");
    }

    #[test]
    fn secret_can_be_read_explicitly() {
        let secret_str = "secret".to_string();
        let secret = Secret(secret_str.clone());
        let secret_real = secret.read();
        assert_eq!(secret_real, secret_str);
    }
}
