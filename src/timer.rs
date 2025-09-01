//! Timer module for measuring execution time at different parts of the codebase.
//!
//! This module provides functionality to:
//! 1. Start and stop named timers
//! 2. Collect execution time statistics
//! 3. Write timer results to a file
//! 4. Global access through a singleton pattern
use anyhow::Result;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use itertools::Itertools;

use crate::args::CGArgs;

// Global timer instance
lazy_static::lazy_static! {
    static ref TIMER: Timer = Timer::new();
}

#[derive(Debug)]
struct TimerData {
    start_time: Option<Instant>,
    elapsed: Duration,
    count: usize,
}

impl TimerData {
    fn new() -> Self {
        Self {
            start_time: None,
            elapsed: Duration::from_secs(0),
            count: 0,
        }
    }
}

#[derive(Debug)]
pub struct Timer {
    timers: Arc<Mutex<HashMap<String, TimerData>>>,
    output_file: Arc<Mutex<Option<String>>>,
}

impl Timer {
    fn new() -> Self {
        Self {
            timers: Arc::new(Mutex::new(HashMap::new())),
            output_file: Arc::new(Mutex::new(None)),
        }
    }

    pub fn init(plugin_args: &CGArgs) {
        // Set up timer output file
        let timer_output_path = plugin_args.timer_output.clone().unwrap_or_else(|| {
            plugin_args
                .output_dir
                .clone()
                .unwrap_or_else(|| PathBuf::from("./target"))
                .join("cg_timing.txt")
        });
        Timer::set_output_file(timer_output_path.to_str().unwrap());
    }

    /// Sets the output file for timer results
    pub fn set_output_file(file_path: &str) {
        let mut output_file = TIMER.output_file.lock().unwrap();
        *output_file = Some(file_path.to_string());
    }

    /// Starts a named timer
    ///
    /// # Arguments
    /// * `name` - The name of the timer to start
    pub fn start(name: &str) {
        let mut timers = TIMER.timers.lock().unwrap();
        let timer = timers
            .entry(name.to_string())
            .or_insert_with(TimerData::new);

        if timer.start_time.is_some() {
            // Timer already running
            tracing::warn!("Timer '{}' already started", name);
            return;
        }

        timer.start_time = Some(Instant::now());
    }

    /// Stops a named timer and records the elapsed time
    ///
    /// # Arguments
    /// * `name` - The name of the timer to stop
    pub fn stop(name: &str) {
        let mut timers = TIMER.timers.lock().unwrap();

        if let Some(timer) = timers.get_mut(name) {
            if let Some(start_time) = timer.start_time {
                let elapsed = start_time.elapsed();
                timer.elapsed += elapsed;
                timer.count += 1;
                timer.start_time = None;

                tracing::debug!(
                    "Timer '{}' stopped. Duration: {:?}, Total: {:?}, Count: {}",
                    name,
                    elapsed,
                    timer.elapsed,
                    timer.count
                );
            } else {
                tracing::warn!("Tried to stop timer '{}' that wasn't started", name);
            }
        } else {
            tracing::warn!("Tried to stop non-existent timer '{}'", name);
        }
    }

    /// Records duration for a named timer without manually starting/stopping
    ///
    /// # Arguments
    /// * `name` - The name of the timer
    /// * `duration` - The duration to record
    #[allow(unused)]
    pub fn record(name: &str, duration: Duration) {
        let mut timers = TIMER.timers.lock().unwrap();
        let timer = timers
            .entry(name.to_string())
            .or_insert_with(TimerData::new);
        timer.elapsed += duration;
        timer.count += 1;

        tracing::debug!(
            "Timer '{}' recorded. Duration: {:?}, Total: {:?}, Count: {}",
            name,
            duration,
            timer.elapsed,
            timer.count
        );
    }

    /// Writes all timer results to the configured output file
    /// If no file is configured, prints to the log instead
    pub fn write_to_file() -> Result<()> {
        let timers = TIMER.timers.lock().unwrap();
        let output_file = TIMER.output_file.lock().unwrap();

        if timers.is_empty() {
            tracing::info!("No timers to write to file");
            return Ok(());
        }

        match output_file.as_ref() {
            Some(file_path) => {
                let file = OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(file_path)?;

                write_timers_to_file(file, &timers)
            }
            None => {
                tracing::info!("No output file configured for timer results");
                for (name, timer) in timers.iter() {
                    tracing::info!(
                        "Timer: {}, Total Duration: {:?}, Count: {}, Avg: {:?}",
                        name,
                        timer.elapsed,
                        timer.count,
                        if timer.count > 0 {
                            timer.elapsed.div_f64(timer.count as f64)
                        } else {
                            Duration::from_secs(0)
                        }
                    );
                }
                Ok(())
            }
        }
    }

    /// Appends timer results to the configured output file
    /// If no file is configured, prints to the log instead
    #[allow(unused)]
    pub fn append_to_file() -> Result<()> {
        let timers = TIMER.timers.lock().unwrap();
        let output_file = TIMER.output_file.lock().unwrap();

        if timers.is_empty() {
            tracing::info!("No timers to append to file");
            return Ok(());
        }

        match output_file.as_ref() {
            Some(file_path) => {
                let file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(file_path)?;

                write_timers_to_file(file, &timers)
            }
            None => {
                tracing::info!("No output file configured for timer results");
                Ok(())
            }
        }
    }

    /// Resets all timers
    #[allow(unused)]
    pub fn reset_all() {
        let mut timers = TIMER.timers.lock().unwrap();
        timers.clear();
    }

    /// Resets a specific timer
    ///
    /// # Arguments
    /// * `name` - The name of the timer to reset
    #[allow(unused)]
    pub fn reset(name: &str) {
        let mut timers = TIMER.timers.lock().unwrap();
        timers.remove(name);
    }
}

fn write_timers_to_file(mut file: File, timers: &HashMap<String, TimerData>) -> Result<()> {
    writeln!(file, "Timer Report - {}", chrono::Local::now())?;
    writeln!(file, "{:-<60}", "")?;
    writeln!(
        file,
        "{:<30} | {:<10} | {:<15} | {:<15}",
        "Timer Name", "Count", "Total (ms)", "Avg (ms)"
    )?;
    writeln!(file, "{:-<60}", "")?;

    let sorted_timers = timers.iter().sorted_by_key(|(name, _)| *name);
    for (name, timer) in sorted_timers {
        let total_ms = timer.elapsed.as_secs_f64() * 1000.0;
        let avg_ms = if timer.count > 0 {
            total_ms / timer.count as f64
        } else {
            0.0
        };

        writeln!(
            file,
            "{:<30} | {:<10} | {:<15.2} | {:<15.2}",
            name, timer.count, total_ms, avg_ms
        )?;
    }

    writeln!(file, "{:-<60}", "")?;
    writeln!(file)?;

    Ok(())
}

/// Measures the execution time of a function and records it
///
/// # Arguments
/// * `name` - The name of the timer
/// * `f` - The function to measure
///
/// # Returns
/// The result of the function
///
/// # Example
/// ```
/// #![feature(rustc_private)]
/// use cg4rs::timer::Timer;
///
/// let result = cg4rs::timer::measure("my_operation", || {
///     // code to measure
///     42
/// });
/// ```
pub fn measure<F, T>(name: &str, f: F) -> T
where
    F: FnOnce() -> T,
{
    Timer::start(name);
    let result = f();
    Timer::stop(name);
    result
}
