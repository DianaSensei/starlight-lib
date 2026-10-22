#![allow(clippy::unwrap_used, clippy::expect_used)]

use starlight_telemetry::{FileLogConfig, TelemetryConfig, init, service_info};

#[test]
fn init_writes_redacted_json_lines_to_rotating_file() {
    let directory =
        std::env::temp_dir().join(format!("starlight-telemetry-{}", std::process::id()));
    let config = TelemetryConfig {
        file: Some(FileLogConfig {
            directory: directory.clone(),
            max_files: 2,
        }),
        ..TelemetryConfig::default()
    };

    let guard = init(service_info!(), &config).unwrap();
    tracing::info_span!("job", job.id = "j-1").in_scope(|| {
        tracing::info!(client_secret = "s3cr3t", "job started");
    });
    drop(guard);

    let file = std::fs::read_dir(&directory)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let content = std::fs::read_to_string(&file).unwrap();
    let line: serde_json::Value = serde_json::from_str(content.lines().last().unwrap()).unwrap();

    assert!(
        file.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("starlight-telemetry")
    );
    assert_eq!(line["message"], "job started");
    assert_eq!(line["client_secret"], "[REDACTED]");
    assert_eq!(line["job.id"], "j-1");
    assert_eq!(line["service.name"], "starlight-telemetry");
    assert_eq!(line["trace_id"].as_str().map(str::len), Some(32));
}

#[test]
fn invalid_sample_ratio_is_rejected() {
    let config = TelemetryConfig {
        trace_sample_ratio: 1.5,
        ..TelemetryConfig::default()
    };

    assert!(init(service_info!(), &config).is_err());
}
