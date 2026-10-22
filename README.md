# starlight-lib

Shared foundation for Starlight services (ADR-0012). Conventions: `CLAUDE.md` and `starlight-common/docs/conventions/`.

| Crate | Provides |
|---|---|
| `starlight-core` | Layered config (`default.yml` → `<profile>.yml` → `SECTION__KEY` env), UUIDv7 id newtypes (`define_id!`, `TenantId`, `UserId`), keyset pagination, health-check trait, shutdown token |
| `starlight-telemetry` | JSON logs to stdout with field redaction and trace ids, optional daily-rotated files, OTLP traces/metrics/logs, W3C propagation |
| `starlight-web` | Axum app builder (request id, span + access log + metrics, timeout, load shedding, body limit, CORS, security headers), RFC 9457 `Problem`, `{"data"}` / `{"data","page"}` responses, Problem-aware extractors, health routes |
| `starlight-auth` | JWT verification via JWKS (EdDSA/ES256/RS256), `AuthContext` extractor with tenant and permissions; `issuer` feature for EdDSA token issuing; `test-util` feature for tests |
| `starlight-db` | Connection pool, tenant-scoped transactions for row-level security, constraint-violation classification, migration helpers |
| `starlight-messaging` | CloudEvents envelope, transactional outbox, JetStream relay |
| `starlight-i18n` | `#[derive(I18nCode)]` |
| `starlight-utils` | Phone number normalization (E.164) |

## Using from a service

Pin an exact revision; bump it through a reviewed PR.

```toml
[dependencies]
starlight-web = { git = "https://github.com/DianaSensei/starlight-lib", rev = "<commit>" }
```

## Development

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

Integration tests need PostgreSQL owned by a **non-superuser** role (superusers bypass row-level security) and NATS
with JetStream; they are `#[ignore]`d unless run with `--include-ignored`:

```bash
export STARLIGHT_TEST_DATABASE_URL=postgres://starlight_app:<password>@localhost:5432/starlight_test
export STARLIGHT_TEST_NATS_URL=nats://localhost:4222
cargo test --workspace --all-features -- --include-ignored
```
