//! SDK runtime — manages connection, registration, collect loop, and shutdown.

use std::time::Duration;

use anyhow::{Context, Result};
use tokio::time::{self, Instant};
use tracing::{debug, error, info, warn};

use telescope_collector_types::ipc::collector_protocol::{RegisterResponse, SubmitResponse};
use telescope_collector_types::ipc::{IpcChannel, IpcRequest, IpcResponse, IpcStream};

use crate::Collector;

/// Env var to override the collector pipe path.
const PIPE_ENV: &str = "TELESCOPE_COLLECTOR_PIPE";

/// Heartbeat interval when no submits happen.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);

/// Maximum backoff between reconnection attempts.
const MAX_BACKOFF: Duration = Duration::from_secs(60);

/// Run the collector's main loop.
pub(crate) async fn run_collector(mut collector: impl Collector) -> Result<()> {
    // Initialize logging.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let manifest = collector.manifest();
    info!(
        name = %manifest.name,
        version = %manifest.version,
        "Collector starting"
    );

    // Connect + register + run loop, with reconnection on failure.
    let mut backoff = Duration::from_millis(500);
    loop {
        match connect_and_run(&mut collector).await {
            Ok(()) => {
                // Clean shutdown (e.g., Ctrl-C or deregister).
                info!("Collector shut down cleanly");
                return Ok(());
            }
            Err(e) => {
                warn!(
                    error = %e,
                    backoff_ms = backoff.as_millis(),
                    "Connection lost, reconnecting"
                );
                time::sleep(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }
    }
}

/// Connect to the service, register, and run the collect loop.
/// Returns `Ok(())` on clean shutdown, `Err` on connection loss.
async fn connect_and_run(collector: &mut impl Collector) -> Result<()> {
    let channel = resolve_channel();
    let mut stream = connect_with_retry(&channel).await?;

    // Register.
    let manifest = collector.manifest();
    let register_resp = register(&mut stream, &manifest).await?;
    info!(
        collector_id = %register_resp.collector_id,
        max_batch_size = register_resp.max_batch_size,
        "Registered with service"
    );

    // Call collector.start() on first successful connection.
    collector
        .start()
        .await
        .context("collector start() failed")?;

    // Run the collect loop with shutdown signal.
    let result = tokio::select! {
        r = collect_loop(&mut stream, collector) => r,
        _ = tokio::signal::ctrl_c() => {
            info!("Received shutdown signal");
            Ok(())
        }
    };

    // Attempt clean deregister (best-effort).
    if let Err(e) = deregister(&mut stream).await {
        debug!(error = %e, "Failed to deregister (connection may be closed)");
    }

    // Call collector.stop().
    if let Err(e) = collector.stop().await {
        warn!(error = %e, "collector stop() failed");
    }

    result
}

/// Resolve the IPC channel to connect to.
fn resolve_channel() -> IpcChannel {
    if let Ok(path) = std::env::var(PIPE_ENV) {
        IpcChannel {
            name: "collector".to_string(),
            path: std::path::PathBuf::from(path),
        }
    } else {
        IpcChannel::collector()
    }
}

/// Connect to the collector pipe with retry.
async fn connect_with_retry(channel: &IpcChannel) -> Result<IpcStream> {
    let mut backoff = Duration::from_millis(100);
    let max_attempts = 10;

    for attempt in 1..=max_attempts {
        match IpcStream::connect(channel).await {
            Ok(stream) => {
                debug!(attempt, "Connected to collector pipe");
                return Ok(stream);
            }
            Err(e) => {
                if attempt == max_attempts {
                    return Err(anyhow::anyhow!(
                        "failed to connect after {max_attempts} attempts: {e}"
                    ));
                }
                debug!(
                    attempt,
                    error = %e,
                    backoff_ms = backoff.as_millis(),
                    "Connection attempt failed, retrying"
                );
                time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(5));
            }
        }
    }
    unreachable!()
}

/// Send `collector.register` and parse the response.
async fn register(
    stream: &mut IpcStream,
    manifest: &crate::CollectorManifest,
) -> Result<RegisterResponse> {
    let request = IpcRequest::new(
        "collector.register",
        serde_json::json!({
            "name": manifest.name,
            "version": manifest.version,
            "description": manifest.description,
            "provenance": {
                "collector_type": manifest.provenance.collector_type,
                "capture_method": manifest.provenance.capture_method,
            },
            "pid": std::process::id(),
            "expected_interval_secs": null,
        }),
    );

    let response = stream
        .call(&request)
        .await
        .context("register call failed")?;
    check_error(&response)?;

    let reg: RegisterResponse = serde_json::from_value(
        response
            .result
            .context("register response missing result")?,
    )
    .context("failed to parse register response")?;
    Ok(reg)
}

/// Main collect loop: collect → batch → submit → respect backpressure → heartbeat.
async fn collect_loop(stream: &mut IpcStream, collector: &mut impl Collector) -> Result<()> {
    let interval = collector.interval();
    let max_batch = telescope_collector_types::ipc::collector_protocol::MAX_BATCH_SIZE;
    let mut last_submit = Instant::now();
    let mut delay_hint = Duration::ZERO;

    loop {
        // Wait for the next collect interval (plus any backpressure delay).
        let wait = interval + delay_hint;
        time::sleep(wait).await;

        // Collect events.
        let events: Vec<crate::EventKind> = match collector.collect().await {
            Ok(events) => events,
            Err(e) => {
                error!(error = %e, "collect() failed, skipping this cycle");
                continue;
            }
        };

        if events.is_empty() {
            // Send heartbeat if no events and it's been a while.
            if last_submit.elapsed() >= HEARTBEAT_INTERVAL {
                heartbeat(stream).await?;
                last_submit = Instant::now();
            }
            continue;
        }

        // Serialize events to JSON values.
        let event_values: Vec<serde_json::Value> = events
            .iter()
            .filter_map(|e| serde_json::to_value(e).ok())
            .collect();

        // Submit in batches of max_batch.
        for chunk in event_values.chunks(max_batch as usize) {
            let resp = submit(stream, chunk).await?;
            debug!(
                accepted = resp.accepted,
                delay_hint_ms = resp.delay_hint_ms,
                "Submitted event batch"
            );
            delay_hint = Duration::from_millis(resp.delay_hint_ms);
            last_submit = Instant::now();
        }
    }
}

/// Send `collector.submit`.
async fn submit(stream: &mut IpcStream, events: &[serde_json::Value]) -> Result<SubmitResponse> {
    let request = IpcRequest::new("collector.submit", serde_json::json!({ "events": events }));

    let response = stream.call(&request).await.context("submit call failed")?;
    check_error(&response)?;

    let resp: SubmitResponse =
        serde_json::from_value(response.result.context("submit response missing result")?)
            .context("failed to parse submit response")?;
    Ok(resp)
}

/// Send `collector.heartbeat`.
async fn heartbeat(stream: &mut IpcStream) -> Result<()> {
    let request = IpcRequest::simple("collector.heartbeat");
    let response = stream
        .call(&request)
        .await
        .context("heartbeat call failed")?;
    check_error(&response)?;
    debug!("Heartbeat sent");
    Ok(())
}

/// Send `collector.deregister`.
async fn deregister(stream: &mut IpcStream) -> Result<()> {
    let request = IpcRequest::simple("collector.deregister");
    let response = stream
        .call(&request)
        .await
        .context("deregister call failed")?;
    check_error(&response)?;
    Ok(())
}

/// Check an IPC response for errors.
fn check_error(response: &IpcResponse) -> Result<()> {
    if let Some(err) = &response.error {
        anyhow::bail!("IPC error {}: {}", err.code, err.message);
    }
    Ok(())
}
