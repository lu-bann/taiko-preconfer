use std::fmt::{Debug, Formatter, Result as FmtResult};

pub struct Password(String);

impl Password {
    pub fn read(&self) -> String {
        self.0.clone()
    }
}

impl Debug for Password {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        write!(f, "Password(********)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_does_not_get_exposed_in_debug_info() {
        let pw_str = "password".to_string();
        let pw = Password(pw_str.clone());
        let pw_dbg = format!("{pw:?}");
        assert_eq!(pw_dbg, "Password(********)");
    }

    #[test]
    fn password_can_be_read_explicitly() {
        let pw_str = "password".to_string();
        let pw = Password(pw_str.clone());
        let pw_real = pw.read();
        assert_eq!(pw_real, pw_str);
    }
}
