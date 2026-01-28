use crate::resource::get_resource;
use opentelemetry_otlp::{LogExporter, WithExportConfig};
use opentelemetry_sdk::logs::SdkLoggerProvider;
use std::fmt::Debug;
use std::net::SocketAddr;
use std::sync::OnceLock;
use time::{OffsetDateTime, format_description};
use time_tz::ToTimezone;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::registry::LookupSpan;

static SDK_LOGGER_PROVIDER: OnceLock<SdkLoggerProvider> = OnceLock::new();
pub fn get_logger_provider() -> &'static SdkLoggerProvider {
    SDK_LOGGER_PROVIDER
        .get()
        .expect("Failed to get a logger provider")
}

pub fn get_or_init_logger_provider(oltp_grpc_url: &str) -> SdkLoggerProvider {
    SDK_LOGGER_PROVIDER
        .get_or_init(|| {
            let exporter = LogExporter::builder()
                .with_tonic()
                .with_endpoint(oltp_grpc_url)
                .build()
                .expect("Failed to create LogExporter");

            SdkLoggerProvider::builder()
                .with_batch_exporter(exporter)
                .with_resource(get_resource())
                .build()
        })
        .clone()
}

#[derive(Debug)]
pub struct CustomLogFormatter;

impl CustomLogFormatter {
    fn new() -> Self {
        CustomLogFormatter {}
    }
}

impl<S, N> FormatEvent<S, N> for CustomLogFormatter
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> std::fmt::Result {
        use opentelemetry::trace::TraceContextExt;

        let system_tz = time_tz::system::get_timezone().expect("Failed to find system timezone");
        let now = OffsetDateTime::now_utc().to_timezone(system_tz);

        let format = format_description::parse("[year]-[month]-[day] [hour]:[minute]:[second].[subsecond digits:3] [offset_hour sign:mandatory]:[offset_minute]").expect("wrong time format");
        let timestamp = now.format(&format).expect("Failed to get a timestamp");
        write!(writer, "{}", timestamp)?;

        let level = event.metadata().level();
        // Format Log Level
        match *level {
            tracing::Level::TRACE => write!(
                writer,
                " {}",
                ansi_term::Colour::Purple.bold().paint(level.as_str())
            ),
            tracing::Level::DEBUG => write!(
                writer,
                " {}",
                ansi_term::Colour::Blue.bold().paint(level.as_str())
            ),
            tracing::Level::INFO => write!(
                writer,
                " {}",
                ansi_term::Colour::Green.bold().paint(level.as_str())
            ),
            tracing::Level::WARN => write!(
                writer,
                " {}",
                ansi_term::Colour::Yellow.bold().paint(level.as_str())
            ),
            tracing::Level::ERROR => write!(
                writer,
                " {}",
                ansi_term::Colour::Red.bold().paint(level.as_str())
            ),
        }?;

        let current_span = tracing::Span::current();
        let trace_id = current_span.context().span().span_context().trace_id();
        let span_id = current_span.context().span().span_context().span_id();

        // Decorate Span info
        // let ctx1 = opentelemetry::Context::current();
        // let trace_id = ctx1.span().span_context().trace_id();
        // let span_id = ctx1.span().span_context().span_id();
        write!(writer, " [{:x},{:x}]", trace_id, span_id)?;

        // get some process information
        let pid = std::process::id();
        let thread = std::thread::current();
        let thread_name = thread.name();
        write!(writer, " [{},{:?}]", pid, thread_name.unwrap())?;

        // Format target
        let target = event.metadata().target();
        write!(
            writer,
            " [{}]",
            ansi_term::Colour::Blue.bold().paint(target)
        )?;

        // Format Module
        let module_split = event.metadata().module_path().expect("").split("::");
        let count = module_split.clone().count();
        let mut module_short = String::new();
        for (pos, module) in module_split.enumerate() {
            if pos == count - 1 {
                module_short.push_str(module);
            } else {
                module_short.push(module.chars().next().unwrap());
                module_short.push_str("::");
            }
        }

        write!(
            writer,
            " {}: ",
            ansi_term::Colour::Yellow.bold().paint(module_short)
        )?;

        ctx.field_format().format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

use axum::body::Body;
use axum::body::Bytes;
use axum::extract::Request;
use axum::http;
use axum::http::{Extensions, StatusCode};
use axum::http::header;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use http_body_util::BodyExt;
use std::time::Instant;
use starlight_protocol::constants::STARLIGHT_REQUEST_ID;

pub async fn print_request_response(
    req: Request,
    next: Next,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let _req_start = Instant::now();

    let req_ext = req.extensions().clone();
    let (parts, body) = req.into_parts();
    let bytes = buffer_and_print_request(&parts, body, req_ext).await?;
    let req = Request::from_parts(parts, Body::from(bytes));

    let res = next.run(req).await;

    let (parts, body) = res.into_parts();
    let bytes = buffer_and_print_response(_req_start, body).await?;
    let res = Response::from_parts(parts, Body::from(bytes));

    Ok(res)
}

async fn buffer_and_print_request<B>(
    parts: &http::request::Parts,
    body: B,
    extension: Extensions
) -> Result<Bytes, (StatusCode, String)>
where
    B: axum::body::HttpBody<Data = Bytes>,
    B::Error: std::fmt::Display,
{
    let headers = &parts.headers;
    info!("----- Receive HTTP {:#?} {:#?}", &parts.method, &parts.uri);
    info!("version: {:#?}", &parts.version);
    // Get from socket (if Axum extensions were set up)
    if let Some(source_addr) = extension.get::<SocketAddr>() {
        info!("source address: {:#?}", source_addr);
    }

    if let Some(forwarded) = parts.headers.get(header::FORWARDED) {
        info!("{}: {:#?}", header::FORWARDED, forwarded.to_str());
    }

    if let Some(user_agent) = headers.get(header::USER_AGENT) {
        info!("{}: {:#?}", header::USER_AGENT, user_agent);
    }

    if let Some(starlight_request_id) = headers.get(STARLIGHT_REQUEST_ID) {
        info!("{}: {:#?}", STARLIGHT_REQUEST_ID, starlight_request_id);
    }

    if let Some(_) = headers.get(header::AUTHORIZATION) {
        info!("{:#?}: \"****************************\"", header::AUTHORIZATION.as_str());
    }


    match body.collect().await {
        Ok(bytes) => {
            let bytes = bytes.to_bytes();
            info!("request payload: {}", String::from_utf8_lossy(&bytes));
            Ok(bytes)
        },
        Err(err) => {
            Err((
                StatusCode::BAD_REQUEST,
                format!("failed to read request body: {}", err),
            ))
        }
    }
}

async fn buffer_and_print_response<B>(
    start_time: Instant,
    body: B,
) -> Result<Bytes, (StatusCode, String)>
where
    B: axum::body::HttpBody<Data = Bytes>,
    B::Error: std::fmt::Display,
{

    match body.collect().await {
        Ok(bytes) => {
            let bytes = bytes.to_bytes();
            info!("response payload: {}", String::from_utf8_lossy(&bytes));
            info!("----- END Request in {:?}", start_time.elapsed());
            Ok(bytes)
        },
        Err(err) => {
            info!("----- END Request in {:?}", start_time.elapsed());
            Err((
                StatusCode::BAD_REQUEST,
                format!("failed to parse response body: {}", err),
            ))
        }
    }
}
