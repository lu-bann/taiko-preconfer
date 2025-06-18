use std::fmt::{Debug, Formatter, Result as FmtResult};

#[derive(Clone)]
pub struct Secret {
    secret: String,
}

impl Secret {
    pub const fn new(secret: String) -> Self {
        Self { secret }
    }
}

impl Secret {
    pub fn read(&self) -> String {
        self.secret.clone()
    }

    pub fn read_slice(&self) -> &str {
        self.secret.as_ref()
    }
}

impl Debug for Secret {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        write!(f, "Secret(secret: ********)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_does_not_get_exposed_in_debug_info() {
        let secret_str = "password".to_string();
        let secret = Secret::new(secret_str.clone());
        let secret_dbg = format!("{secret:?}");
        assert_eq!(secret_dbg, "Secret(secret: ********)");
    }

    #[test]
    fn secret_can_be_read_explicitly() {
        let secret_raw_str = "secret";
        let secret_str = secret_raw_str.to_string();
        let secret = Secret::new(secret_str.clone());
        let secret_real = secret.read();
        assert_eq!(secret_real, secret_str);
        let secret_real_raw = secret.read_slice();
        assert_eq!(secret_real_raw, secret_raw_str);
    }
}
