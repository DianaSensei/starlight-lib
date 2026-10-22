//! Configuration, identifiers, pagination, health and shutdown primitives shared by Starlight services.

pub mod config;
pub mod health;
pub mod id;
pub mod pagination;
pub mod shutdown;

pub use uuid;

crate::define_id!(
    /// Tenant (organization or consuming system) that owns data.
    TenantId
);
crate::define_id!(
    /// End user or operator account.
    UserId
);
