use starlight_i18n::I18nError;
use std::any::Any;

/// Helper enum to represent i18n parameters
/// This must match what the macro generates
pub enum I18nParam {
    Tuple(Vec<Box<dyn Any>>),
    Struct(Vec<(&'static str, Box<dyn Any>)>),
}

/// Test enum with unit variants only
#[derive(I18nError)]
pub enum SimpleError {
    #[i18n("error.not_found")]
    NotFound,
    #[i18n("error.unauthorized")]
    Unauthorized,
}

/// Test enum with unnamed fields (tuple variant)
#[derive(I18nError, Clone)]
pub enum TupleError {
    #[i18n("error.invalid_id")]
    InvalidId(i32),
    #[i18n("error.range")]
    OutOfRange(i32, i32),
}

/// Test enum with named fields (struct variant)
#[derive(I18nError, Clone)]
pub enum StructError {
    #[i18n("error.validation")]
    ValidationFailed { field: String, message: String },
    #[i18n("error.missing_field")]
    MissingField { name: String },
}

/// Test enum with mixed variants
#[derive(I18nError, Clone)]
pub enum MixedError {
    #[i18n("error.simple")]
    Simple,
    #[i18n("error.with_code")]
    WithCode(u32),
    #[i18n("error.detailed")]
    Detailed { code: u32, reason: String },
}

#[test]
fn test_unit_variant_key() {
    let error = SimpleError::NotFound;
    assert_eq!(error.get_key(), "error.not_found");

    let error = SimpleError::Unauthorized;
    assert_eq!(error.get_key(), "error.unauthorized");
}

#[test]
fn test_unit_variant_param() {
    let error = SimpleError::NotFound;
    assert!(error.get_param().is_none());

    let error = SimpleError::Unauthorized;
    assert!(error.get_param().is_none());
}

#[test]
fn test_tuple_variant_key() {
    let error = TupleError::InvalidId(42);
    assert_eq!(error.get_key(), "error.invalid_id");

    let error = TupleError::OutOfRange(1, 100);
    assert_eq!(error.get_key(), "error.range");
}

#[test]
fn test_tuple_variant_param() {
    let error = TupleError::InvalidId(42);
    let param = error.get_param();
    assert!(param.is_some());

    if let Some(I18nParam::Tuple(values)) = param {
        assert_eq!(values.len(), 1);
        assert_eq!(*values[0].downcast_ref::<i32>().unwrap(), 42);
    } else {
        panic!("Expected Tuple param");
    }

    let error = TupleError::OutOfRange(1, 100);
    if let Some(I18nParam::Tuple(values)) = error.get_param() {
        assert_eq!(values.len(), 2);
        assert_eq!(*values[0].downcast_ref::<i32>().unwrap(), 1);
        assert_eq!(*values[1].downcast_ref::<i32>().unwrap(), 100);
    } else {
        panic!("Expected Tuple param");
    }
}

#[test]
fn test_struct_variant_key() {
    let error = StructError::ValidationFailed {
        field: "email".to_string(),
        message: "invalid format".to_string(),
    };
    assert_eq!(error.get_key(), "error.validation");

    let error = StructError::MissingField {
        name: "username".to_string(),
    };
    assert_eq!(error.get_key(), "error.missing_field");
}

#[test]
fn test_struct_variant_param() {
    let error = StructError::MissingField {
        name: "username".to_string(),
    };
    let param = error.get_param();
    assert!(param.is_some());

    if let Some(I18nParam::Struct(pairs)) = param {
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].0, "name");
        assert_eq!(
            pairs[0].1.downcast_ref::<String>().unwrap(),
            "username"
        );
    } else {
        panic!("Expected Struct param");
    }
}

#[test]
fn test_mixed_variant_keys() {
    assert_eq!(MixedError::Simple.get_key(), "error.simple");
    assert_eq!(MixedError::WithCode(500).get_key(), "error.with_code");
    assert_eq!(
        MixedError::Detailed {
            code: 404,
            reason: "not found".to_string()
        }
        .get_key(),
        "error.detailed"
    );
}

#[test]
fn test_mixed_variant_params() {
    // Unit variant should have no params
    assert!(MixedError::Simple.get_param().is_none());

    // Tuple variant should have Tuple param
    if let Some(I18nParam::Tuple(values)) = MixedError::WithCode(500).get_param() {
        assert_eq!(values.len(), 1);
        assert_eq!(*values[0].downcast_ref::<u32>().unwrap(), 500);
    } else {
        panic!("Expected Tuple param for WithCode");
    }

    // Struct variant should have Struct param
    let detailed = MixedError::Detailed {
        code: 404,
        reason: "not found".to_string(),
    };
    if let Some(I18nParam::Struct(pairs)) = detailed.get_param() {
        assert_eq!(pairs.len(), 2);
        // Check that we have the expected fields (order may vary)
        let code_pair = pairs.iter().find(|(k, _)| *k == "code");
        let reason_pair = pairs.iter().find(|(k, _)| *k == "reason");

        assert!(code_pair.is_some());
        assert!(reason_pair.is_some());

        assert_eq!(*code_pair.unwrap().1.downcast_ref::<u32>().unwrap(), 404);
        assert_eq!(
            reason_pair.unwrap().1.downcast_ref::<String>().unwrap(),
            "not found"
        );
    } else {
        panic!("Expected Struct param for Detailed");
    }
}
