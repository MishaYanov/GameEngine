//! A small reproducible measurement, not a performance guarantee.
use engine_logger::{Config, StorageConfig};
use std::time::{Duration, Instant};
fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let directory = std::env::var_os("ENGINE_LOG_DIR")
        .ok_or("set ENGINE_LOG_DIR to the measurement output directory")?;
    let mut config = Config::new("logger-measure");
    config.console = false;
    let mut storage = StorageConfig::new(directory);
    storage.queue_capacity = 8192;
    config.storage = Some(storage);
    let _logging = engine_logger::init(config)?;
    let started = Instant::now();
    for i in 0..5000 {
        engine_logger::info!(target: "measurement", "normal event {i}");
    }
    let normal = started.elapsed();
    let started = Instant::now();
    engine_logger::error!("error trace measurement");
    let error = started.elapsed();
    let started = Instant::now();
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                for _ in 0..5000 {
                    engine_logger::info!(target: "measurement", "burst event");
                }
            });
        }
    });
    let burst = started.elapsed();
    assert!(engine_logger::shutdown(Duration::from_secs(10)));
    let health = engine_logger::snapshot(Default::default()).health;
    println!(
        "normal_5000={normal:?} error_with_trace={error:?} burst_20000={burst:?} disk_drops={} memory_drops={} persistence_error={:?}",
        health.dropped_persistence, health.dropped_memory, health.persistence_error
    );
    Ok(())
}
