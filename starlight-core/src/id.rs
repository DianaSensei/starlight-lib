/// Declares a UUIDv7-backed identifier newtype so different entity ids cannot be mixed up.
/// The calling crate must depend on `serde`.
#[macro_export]
macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            ::serde::Serialize,
            ::serde::Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name($crate::uuid::Uuid);

        impl $name {
            /// Generates a new time-ordered identifier.
            pub fn generate() -> Self {
                Self($crate::uuid::Uuid::now_v7())
            }

            pub const fn from_uuid(value: $crate::uuid::Uuid) -> Self {
                Self(value)
            }

            pub const fn as_uuid(&self) -> $crate::uuid::Uuid {
                self.0
            }
        }

        impl From<$crate::uuid::Uuid> for $name {
            fn from(value: $crate::uuid::Uuid) -> Self {
                Self(value)
            }
        }

        impl From<$name> for $crate::uuid::Uuid {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }

        impl std::str::FromStr for $name {
            type Err = $crate::uuid::Error;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                value.parse().map(Self)
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use crate::TenantId;

    #[test]
    fn generated_ids_are_time_ordered_v7() {
        let first = TenantId::generate();
        let second = TenantId::generate();

        assert_eq!(first.as_uuid().get_version_num(), 7);
        assert!(first < second);
    }

    #[test]
    fn id_round_trips_through_string_and_json() {
        let id = TenantId::generate();

        let parsed: TenantId = id.to_string().parse().unwrap();
        let json = serde_json::to_string(&id).unwrap();

        assert_eq!(parsed, id);
        assert_eq!(json, format!("\"{id}\""));
    }
}
