# Starlight — hướng dẫn cho Claude

> File này được đồng bộ từ `starlight-common/shared/CLAUDE.md` (`scripts/sync-shared-files.sh`). Sửa ở đó, không sửa bản sao.
> Tài liệu đầy đủ: `starlight-common/docs/` — ADR ở `docs/adr/`, convention ở `docs/conventions/`.

Starlight là hệ sinh thái dịch vụ nền (backbone) dùng chung cho nhiều hệ thống: định danh, tenant/IAM, merchant,
catalog, storage, geolocation, notification.

## Tech stack (ADR-0017 — đã chốt)

Rust stable (edition 2024) · Tokio · Axum · SeaQL (SeaORM 2.x, sea-orm-migration, SeaQuery) · NATS JetStream ·
Redis · PostgreSQL-compatible · OpenTelemetry (logs, metrics, traces) · chạy hoàn toàn trong container.
Không thêm framework/crate thay thế các lựa chọn này khi chưa có ADR.

## Comment — quy tắc bắt buộc

- Ưu tiên code tự giải thích: tên rõ, hàm nhỏ, kiểu chặt. Không viết comment để bù cho code khó đọc — sửa code.
- Không comment bừa bãi, không dài dòng. Chỉ comment khi cần nói **tại sao** hoặc ràng buộc mà code không thể hiện được; tối đa 1–2 dòng, ngắn gọn, xúc tích.
- Không comment diễn giải lại code, không banner, không lịch sử thay đổi, không để lại code bị comment-out.
- `///` cho API công khai: một dòng tóm tắt. `TODO` phải kèm issue: `TODO(#123): …`.
- Comment, tên định danh, message log, commit message viết bằng tiếng Anh.

## Code

- Clean code: hàm làm một việc (~40 dòng), ≤ 5 tham số, không tham số bool làm cờ; `pub` tối thiểu.
- Naming: `snake_case` (hàm, biến, module, file, cột DB, JSON), `UpperCamelCase` (type, trait, variant; viết tắt như một từ: `HttpServer`), `SCREAMING_SNAKE_CASE` (hằng); bool `is_/has_/can_`; không `get_` cho getter.
- Wrapping: `cargo fmt` (`max_width = 100`); điều kiện dài tách thành biến bool có tên; lỗi bọc có ngữ cảnh bằng `thiserror`, dùng `?`; newtype cho ID (`StoreId(Uuid)`), `SecretString` cho bí mật, không dùng float cho tiền.
- Điều kiện: luôn có `{}`; "3 ngôi" = `if` biểu thức một dòng (`let x = if c { a } else { b };`) hoặc `then_some`/`map_or`/`unwrap_or`; không lồng; ≥ 3 nhánh dùng `match` (không `_` cho enum domain); thoát sớm bằng `let … else`.
- Cấm `unwrap`/`expect` ngoài test và khởi động; cấm `println!`/`dbg!`; cấm `unsafe`, `static mut`.
- Kiến trúc hexagonal: `domain/` (thuần) → `application/` (use case, port trait) → `adapters/{http,persistence,messaging}`.

## Log & trace

- Chỉ `tracing`; log có cấu trúc: `info!(store.id = %id, "store created")` — message ngắn, cố định, dữ liệu trong field.
- `#[instrument(name = "<module>.<operation>", skip_all, fields(...))]` ở use case và adapter.
- Lỗi log đúng một lần tại nơi xử lý; 4xx là `info`/`warn`, 5xx là `error`.
- Cấm log mật khẩu, OTP, token, secret, header `Authorization`/`Cookie`, body request/response; PII chỉ log ID.

## REST API

- `/api/v1/<danh-từ-số-nhiều-kebab-case>`; method & status đúng ngữ nghĩa (201 + `Location`, 204 cho DELETE).
- Request: body là tài nguyên `snake_case`, không envelope; `idempotency-key` cho POST tạo; `If-Match` cho cập nhật.
- Response: `{"data": …}` · danh sách `{"data": [...], "page": {"limit", "next_cursor", "has_more"}}` · lỗi `application/problem+json` có `code`, `request_id`, `trace_id`, `errors[]`.
- Danh sách dùng cursor, `limit` ≤ 100; ID là UUIDv7; thời gian RFC 3339 UTC; enum là chuỗi; tenant lấy từ token.

## Thread safety

- State qua `Arc<AppState>`; không global khả biến; trait dùng chung `Send + Sync`.
- Không giữ lock qua `.await`; tác vụ nặng/đồng bộ dùng `spawn_blocking`; channel có giới hạn; task spawn có `CancellationToken`.
- Đúng đắn giữa replica dựa vào DB: unique constraint, transaction + outbox, optimistic locking (`version`), consumer idempotent.

## Bảo mật & cấu hình

- Không bao giờ commit secret; cấu hình nhạy cảm qua biến môi trường `SECTION__KEY` (xem `.env.example`).
- Gitleaks chạy trong CI và pre-commit — không tắt, không thêm allowlist cho secret thật.

## Áp dụng cho code hiện có

Code cũ chưa theo hết convention. Code mới và phần đang sửa **phải** theo; không refactor phần không liên quan
trong cùng thay đổi — làm ở commit/PR riêng.

## Trước khi commit

`cargo fmt --all` · `cargo clippy --all-targets` (không thêm warning mới; repo đã bật lint thì `-D warnings`) ·
`cargo test`. Commit theo Conventional Commits (`feat:`, `fix:`, `refactor:`, `docs:`, `ci:`, `chore:`), mỗi
commit một mục đích.
