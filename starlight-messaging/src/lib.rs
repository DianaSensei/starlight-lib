//! CloudEvents over NATS JetStream with a transactional outbox (ADR-0007).

mod event;
mod outbox;
mod relay;

pub use async_nats;
pub use event::{CloudEvent, NewEvent};
pub use outbox::{OUTBOX_TABLE_SQL, enqueue};
pub use relay::{OutboxRelay, RelayConfig, RelayError, ensure_stream};
