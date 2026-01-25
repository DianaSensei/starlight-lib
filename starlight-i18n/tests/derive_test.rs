use starlight_i18n::I18nCode;

/// Test enum with unit variants only
#[derive(I18nCode)]
pub enum SimpleError {
    #[i18n_code("error.not_found")]
    NotFound,
    #[i18n_code("error.unauthorized")]
    Unauthorized,
}

/// Test enum with unnamed fields (tuple variant)
#[derive(I18nCode, Clone)]
pub enum TupleError {
    #[i18n_code("error.invalid_id")]
    InvalidId(i32),
    #[i18n_code("error.range")]
    OutOfRange(i32, i32),
}

/// Test enum with named fields (struct variant)
#[derive(I18nCode, Clone)]
pub enum StructError {
    #[i18n_code("error.validation")]
    ValidationFailed { field: String, message: String },
    #[i18n_code("error.missing_field")]
    MissingField { name: String },
}

/// Test enum with mixed variants
#[derive(I18nCode, Clone)]
pub enum MixedError {
    #[i18n_code("error.simple")]
    Simple,
    #[i18n_code("error.with_code")]
    WithCode(u32),
    #[i18n_code("error.detailed")]
    Detailed { code: u32, reason: String },
}

#[test]
fn test_unit_variant_key() {
    let error = SimpleError::NotFound;
    assert_eq!(error.get_i18n_code(), "error.not_found");

    let error = SimpleError::Unauthorized;
    assert_eq!(error.get_i18n_code(), "error.unauthorized");
}

#[test]
fn test_tuple_variant_key() {
    let error = TupleError::InvalidId(42);
    assert_eq!(error.get_i18n_code(), "error.invalid_id");

    let error = TupleError::OutOfRange(1, 100);
    assert_eq!(error.get_i18n_code(), "error.range");
}

#[test]
fn test_struct_variant_key() {
    let error = StructError::ValidationFailed {
        field: "email".to_string(),
        message: "invalid format".to_string(),
    };
    assert_eq!(error.get_i18n_code(), "error.validation");

    let error = StructError::MissingField {
        name: "username".to_string(),
    };
    assert_eq!(error.get_i18n_code(), "error.missing_field");
}

#[test]
fn test_mixed_variant_keys() {
    assert_eq!(MixedError::Simple.get_i18n_code(), "error.simple");
    assert_eq!(MixedError::WithCode(500).get_i18n_code(), "error.with_code");
    assert_eq!(
        MixedError::Detailed {
            code: 404,
            reason: "not found".to_string()
        }
        .get_i18n_code(),
        "error.detailed"
    );
}
