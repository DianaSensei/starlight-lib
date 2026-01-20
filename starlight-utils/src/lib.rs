pub mod phone;

pub use phone::{
    detect_country, is_valid_e164, normalize_phone, normalize_vn_phone, PhoneNumber,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_vietnamese_numbers() {
        let vn1 = normalize_vn_phone("0912 345 678").unwrap();
        assert_eq!(vn1, "+84912345678");

        let vn2 = normalize_vn_phone("+84 912-345-678").unwrap();
        assert_eq!(vn2, "+84912345678");

        let vn3 = normalize_vn_phone("0084 912 345 678").unwrap();
        assert_eq!(vn3, "+84912345678");

        // Landline-like patterns should also normalize (lengths may vary by area code)
        let vn4 = normalize_vn_phone("028 3822 8899").unwrap();
        assert!(is_valid_e164(&vn4));
        assert!(vn4.starts_with("+84"));
    }

    #[test]
    fn normalize_other_countries() {
        // US
        let us = normalize_phone("(415) 555-2671", "US").unwrap();
        assert_eq!(us.e164, "+14155552671");
        assert_eq!(detect_country(&us.e164).unwrap(), "US");

        // Singapore
        let sg = normalize_phone("9123 4567", "SG").unwrap();
        assert_eq!(sg.e164, "+6591234567");
        assert_eq!(detect_country(&sg.e164).unwrap(), "SG");

        // Using international '00' prefix should ignore default country
        let intl = normalize_phone("0084 912 345 678", "US").unwrap();
        assert_eq!(intl.e164, "+84912345678");
        assert_eq!(detect_country(&intl.e164).unwrap(), "VN");
    }

    #[test]
    fn rejects_invalid_numbers() {
        // Not a number
        assert!(normalize_phone("abc-xyz", "VN").is_none());
        // Invalid E.164 characters
        assert!(!is_valid_e164("+84-912345678"));
    }
}
