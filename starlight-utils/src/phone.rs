/// Simple phone normalization utilities without external dependencies.
///
/// Main goals:
/// - Normalize phone numbers to E.164: +<country_code><national_number>
/// - Handle common user input variants (spaces, dashes, parentheses)
/// - Understand Vietnamese numbers well, and provide a generic path for other countries
/// - Detect country from E.164 (best-effort for common country codes)
///
/// Note: This is a lightweight heuristic implementation. It does not fully validate
/// numbering plans for all countries.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhoneNumber {
    /// Original input
    pub raw: String,
    /// Normalized E.164 number (e.g., +84912345678)
    pub e164: String,
    /// Numeric country calling code (e.g., "84")
    pub country_code: String,
    /// National significant number (without country code, usually without trunk '0')
    pub national_number: String,
    /// ISO 3166-1 alpha-2 when known (e.g., "VN")
    pub iso_country: Option<&'static str>,
}

/// Normalize a phone number into E.164 using a default country hint.
/// The default_country can be:
/// - ISO code like "VN", "US", "SG"
/// - Country calling code like "84", "1", "65"
/// - Or with a '+' like "+84"
///
/// Returns a structured result with E.164 and decomposition on success.
pub fn normalize_phone(input: &str, default_country: &str) -> Option<PhoneNumber> {
    let raw = input.to_string();
    let s = strip_non_digits_keep_plus(input);

    if s.is_empty() {
        return None;
    }

    // If it's already using '+' form, parse directly
    if s.starts_with('+') {
        let digits = &s[1..];
        if !digits.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let (cc, iso) = match_country_code_prefix(digits)?;
        let nsn = digits.get(cc.len()..)?.to_string();

        // Sometimes users might include a trunk '0' after the country code;
        // for certain countries we can trim it.
        let nsn = if iso.map(is_trunk_zero_country).unwrap_or(false) && nsn.starts_with('0') {
            nsn.trim_start_matches('0').to_string()
        } else {
            nsn
        };

        let e164 = format!("+{}{}", cc, nsn);
        if !is_valid_e164(&e164) {
            return None;
        }

        return Some(PhoneNumber {
            raw,
            e164,
            country_code: cc.to_string(),
            national_number: nsn,
            iso_country: iso,
        });
    }

    // International prefix starting with "00"
    if s.starts_with("00") {
        // Convert to '+' and re-run
        let plus_form = format!("+{}", &s[2..]);
        return normalize_phone(&plus_form, default_country);
    }

    // Local/national number: use default_country
    let (cc, iso) = resolve_country_hint(default_country)?;
    // Keep only digits (no '+')
    let mut nsn: String = s.chars().filter(|c| c.is_ascii_digit()).collect();

    // Remove trunk leading '0' for specific countries (e.g., VN, GB, DE, FR, IT, TH, MY, ID, JP, KR)
    if iso.map(is_trunk_zero_country).unwrap_or(false) {
        while nsn.starts_with('0') {
            // Be conservative: remove only the first leading '0'
            nsn.remove(0);
            break;
        }
    }

    if nsn.is_empty() {
        return None;
    }

    let e164 = format!("+{}{}", cc, nsn);
    if !is_valid_e164(&e164) {
        return None;
    }

    Some(PhoneNumber {
        raw,
        e164,
        country_code: cc.to_string(),
        national_number: nsn,
        iso_country: iso,
    })
}

/// Convenience: Normalize assuming Vietnam as default country. Returns E.164 on success.
pub fn normalize_vn_phone(input: &str) -> Option<String> {
    normalize_phone(input, "VN").map(|p| p.e164)
}

/// Best-effort detection of ISO country code from an E.164 number.
pub fn detect_country(e164: &str) -> Option<&'static str> {
    if !is_valid_e164(e164) {
        return None;
    }
    let digits = &e164[1..];
    let (cc, iso) = match_country_code_prefix(digits)?;
    if iso.is_some() {
        iso
    } else {
        // If we don't have ISO mapping, still return something meaningful if possible.
        code_to_iso(cc)
    }
}

/// Check whether a string is a valid E.164 representation (syntax only).
pub fn is_valid_e164(s: &str) -> bool {
    if !s.starts_with('+') {
        return false;
    }
    let digits = &s[1..];
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    // E.164 max length is 15 digits (excluding '+'); keep a sensible minimum too.
    let len = digits.len();
    (len >= 7) && (len <= 15)
}

fn strip_non_digits_keep_plus(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for (i, ch) in input.chars().enumerate() {
        if ch.is_ascii_digit() {
            out.push(ch);
        } else if ch == '+' && i == 0 {
            out.push(ch);
        } else {
            // skip
        }
    }
    out
}

fn resolve_country_hint(hint: &str) -> Option<(&'static str, Option<&'static str>)> {
    let up = hint.trim().to_ascii_uppercase();
    // Accept "+84" forms
    if up.starts_with('+') && up[1..].chars().all(|c| c.is_ascii_digit()) {
        let code = &up[1..];
        if !code.is_empty() && code.len() <= 3 {
            return Some((code_to_cow_static(code)?, code_to_iso(code)));
        }
        return None;
    }

    // Accept "84" forms
    if up.chars().all(|c| c.is_ascii_digit()) && !up.is_empty() && up.len() <= 3 {
        return Some((code_to_cow_static(&up)?, code_to_iso(&up)));
    }

    // Accept ISO alpha-2 forms
    match up.as_str() {
        "VN" => Some(("84", Some("VN"))),
        "US" => Some(("1", Some("US"))),   // ambiguous (US/CA); treat as US by default
        "CA" => Some(("1", Some("CA"))),
        "SG" => Some(("65", Some("SG"))),
        "TH" => Some(("66", Some("TH"))),
        "CN" => Some(("86", Some("CN"))),
        "JP" => Some(("81", Some("JP"))),
        "KR" => Some(("82", Some("KR"))),
        "GB" => Some(("44", Some("GB"))),
        "DE" => Some(("49", Some("DE"))),
        "FR" => Some(("33", Some("FR"))),
        "AU" => Some(("61", Some("AU"))),
        "NZ" => Some(("64", Some("NZ"))),
        "MY" => Some(("60", Some("MY"))),
        "ID" => Some(("62", Some("ID"))),
        "PH" => Some(("63", Some("PH"))),
        "ES" => Some(("34", Some("ES"))),
        "IT" => Some(("39", Some("IT"))),
        "RU" => Some(("7", Some("RU"))),
        "BR" => Some(("55", Some("BR"))),
        "MX" => Some(("52", Some("MX"))),
        "IN" => Some(("91", Some("IN"))),
        "HK" => Some(("852", Some("HK"))),
        "MO" => Some(("853", Some("MO"))),
        "TW" => Some(("886", Some("TW"))),
        _ => None,
    }
}

fn is_trunk_zero_country(iso: &str) -> bool {
    matches!(
        iso,
        "VN" | "GB" | "DE" | "FR" | "IT" | "TH" | "MY" | "ID" | "JP" | "KR"
    )
}

/// Given digits after '+', find the longest matching country calling code and ISO if known.
fn match_country_code_prefix(digits_after_plus: &str) -> Option<(&'static str, Option<&'static str>)> {
    // Country calling codes are 1 to 3 digits. Match the longest possible.
    for len in [3usize, 2, 1] {
        if digits_after_plus.len() < len {
            continue;
        }
        let cand = &digits_after_plus[..len];
        if let Some(iso) = code_to_iso(cand) {
            return Some((code_to_cow_static(cand)?, Some(iso)));
        }
        // Even if ISO unknown, if it's a plausible code from our list, accept it
        if is_known_code(cand) {
            return Some((code_to_cow_static(cand)?, None));
        }
    }
    None
}

fn is_known_code(code: &str) -> bool {
    matches!(
        code,
        "1" | "7" | "33" | "34" | "39" | "44" | "49" | "52" | "55" | "60" | "61" | "62" | "63"
            | "64" | "65" | "66" | "81" | "82" | "84" | "86" | "852" | "853" | "886" | "91"
    )
}

fn code_to_iso(code: &str) -> Option<&'static str> {
    match code {
        "84" => Some("VN"),
        "1" => Some("US"),   // could also be CA et al; simplified
        "44" => Some("GB"),
        "49" => Some("DE"),
        "33" => Some("FR"),
        "81" => Some("JP"),
        "82" => Some("KR"),
        "65" => Some("SG"),
        "66" => Some("TH"),
        "86" => Some("CN"),
        "852" => Some("HK"),
        "853" => Some("MO"),
        "886" => Some("TW"),
        "62" => Some("ID"),
        "60" => Some("MY"),
        "63" => Some("PH"),
        "61" => Some("AU"),
        "64" => Some("NZ"),
        "34" => Some("ES"),
        "39" => Some("IT"),
        "7" => Some("RU"),
        "55" => Some("BR"),
        "52" => Some("MX"),
        "91" => Some("IN"),
        _ => None,
    }
}

fn code_to_cow_static(code: &str) -> Option<&'static str> {
    // Lift numeric literals into 'static; we only support up to 3 digits.
    match code {
        "1" => Some("1"),
        "7" => Some("7"),
        "33" => Some("33"),
        "34" => Some("34"),
        "39" => Some("39"),
        "44" => Some("44"),
        "49" => Some("49"),
        "52" => Some("52"),
        "55" => Some("55"),
        "60" => Some("60"),
        "61" => Some("61"),
        "62" => Some("62"),
        "63" => Some("63"),
        "64" => Some("64"),
        "65" => Some("65"),
        "66" => Some("66"),
        "81" => Some("81"),
        "82" => Some("82"),
        "84" => Some("84"),
        "86" => Some("86"),
        "852" => Some("852"),
        "853" => Some("853"),
        "886" => Some("886"),
        "91" => Some("91"),
        _ => None,
    }
}
