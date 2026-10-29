//! Synthetic local Argon2 timing only; no user credentials or vault file.
use std::time::Instant;

use codeward_core::crypto::derive_master_key;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let salt = [0xA5; 16];
    for attempt in 0..6 {
        let start = Instant::now();
        let key = derive_master_key(b"synthetic benchmark input", &salt)?;
        drop(key);
        println!(
            "attempt {}: {:.3} ms",
            attempt + 1,
            start.elapsed().as_secs_f64() * 1000.0
        );
    }
    Ok(())
}
