//! Bidirectional Relay

use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, AsyncReadExt, AsyncWriteExt};
use tokio::time::Instant;
use tracing::{debug, trace, warn, info};
use crate::error::Result;
use crate::stats::Stats;
use crate::stream::BufferPool;
use std::sync::atomic::{AtomicU64, Ordering};

// Activity timeout for iOS compatibility (30 minutes)
const ACTIVITY_TIMEOUT_SECS: u64 = 1800;

/// Relay data bidirectionally between client and server
pub async fn relay_bidirectional<CR, CW, SR, SW>(
    mut client_reader: CR,
    mut client_writer: CW,
    mut server_reader: SR,
    mut server_writer: SW,
    user: &str,
    stats: Arc<Stats>,
    buffer_pool: Arc<BufferPool>,
) -> Result<()>
where
    CR: AsyncRead + Unpin + Send + 'static,
    CW: AsyncWrite + Unpin + Send + 'static,
    SR: AsyncRead + Unpin + Send + 'static,
    SW: AsyncWrite + Unpin + Send + 'static,
{
    let user_c2s = user.to_string();
    let user_s2c = user.to_string();
    
    let stats_c2s = Arc::clone(&stats);
    let stats_s2c = Arc::clone(&stats);
    
    let c2s_bytes = Arc::new(AtomicU64::new(0));
    let s2c_bytes = Arc::new(AtomicU64::new(0));
    let c2s_bytes_clone = Arc::clone(&c2s_bytes);
    let s2c_bytes_clone = Arc::clone(&s2c_bytes);
    
    let activity_timeout = Duration::from_secs(ACTIVITY_TIMEOUT_SECS);
    
    let pool_c2s = buffer_pool.clone();
    let pool_s2c = buffer_pool.clone();
    
    // Client -> Server task
    let c2s = tokio::spawn(async move {
        // Get buffer from pool
        let mut buf = pool_c2s.get();
        let mut total_bytes = 0u64;
        let mut prev_total_bytes = 0u64;
        let mut msg_count = 0u64;
        let mut last_activity = Instant::now();
        let mut last_log = Instant::now();
        
        loop {
            // Read with timeout
            let read_result = tokio::time::timeout(
                activity_timeout,
                client_reader.read(&mut buf)
            ).await;
            
            match read_result {
                Err(_) => {
                    warn!(
                        user = %user_c2s,
                        total_bytes = total_bytes,
                        msgs = msg_count,
                        idle_secs = last_activity.elapsed().as_secs(),
                        "Activity timeout (C->S) - no data received"
                    );
                    let _ = server_writer.shutdown().await;
                    break;
                }
                
                Ok(Ok(0)) => {
                    debug!(
                        user = %user_c2s, 
                        total_bytes = total_bytes,
                        msgs = msg_count,
                        "Client closed connection (C->S)"
                    );
                    let _ = server_writer.shutdown().await;
                    break;
                }
                
                Ok(Ok(n)) => {
                    total_bytes += n as u64;
                    msg_count += 1;
                    last_activity = Instant::now();
                    c2s_bytes_clone.store(total_bytes, Ordering::Relaxed);
                    
                    stats_c2s.add_user_octets_from(&user_c2s, n as u64);
                    stats_c2s.increment_user_msgs_from(&user_c2s);
                    
                    trace!(
                        user = %user_c2s,
                        bytes = n,
                        total = total_bytes,
                        "C->S data"
                    );
                    
                    // Log activity every 10 seconds with correct rate
                    let elapsed = last_log.elapsed();
                    if elapsed > Duration::from_secs(10) {
                        let delta = total_bytes - prev_total_bytes;
                        let rate = delta as f64 / elapsed.as_secs_f64();
                        
                        // Changed to DEBUG to reduce log spam
                        debug!(
                            user = %user_c2s,
                            total_bytes = total_bytes,
                            msgs = msg_count,
                            rate_kbps = (rate / 1024.0) as u64,
                            "C->S transfer in progress"
                        );
                        
                        last_log = Instant::now();
                        prev_total_bytes = total_bytes;
                    }
                    
                    if let Err(e) = server_writer.write_all(&buf[..n]).await {
                        debug!(user = %user_c2s, error = %e, "Failed to write to server");
                        break;
                    }
                    if let Err(e) = server_writer.flush().await {
                        debug!(user = %user_c2s, error = %e, "Failed to flush to server");
                        break;
                    }
                }
                
                Ok(Err(e)) => {
                    debug!(user = %user_c2s, error = %e, total_bytes = total_bytes, "Client read error");
                    break;
                }
            }
        }
    });
    
    // Server -> Client task
    let s2c = tokio::spawn(async move {
        // Get buffer from pool
        let mut buf = pool_s2c.get();
        let mut total_bytes = 0u64;
        let mut prev_total_bytes = 0u64;
        let mut msg_count = 0u64;
        let mut last_activity = Instant::now();
        let mut last_log = Instant::now();
        
        loop {
            let read_result = tokio::time::timeout(
                activity_timeout,
                server_reader.read(&mut buf)
            ).await;
            
            match read_result {
                Err(_) => {
                    warn!(
                        user = %user_s2c,
                        total_bytes = total_bytes,
                        msgs = msg_count,
                        idle_secs = last_activity.elapsed().as_secs(),
                        "Activity timeout (S->C) - no data received"
                    );
                    let _ = client_writer.shutdown().await;
                    break;
                }
                
                Ok(Ok(0)) => {
                    debug!(
                        user = %user_s2c,
                        total_bytes = total_bytes,
                        msgs = msg_count,
                        "Server closed connection (S->C)"
                    );
                    let _ = client_writer.shutdown().await;
                    break;
                }
                
                Ok(Ok(n)) => {
                    total_bytes += n as u64;
                    msg_count += 1;
                    last_activity = Instant::now();
                    s2c_bytes_clone.store(total_bytes, Ordering::Relaxed);
                    
                    stats_s2c.add_user_octets_to(&user_s2c, n as u64);
                    stats_s2c.increment_user_msgs_to(&user_s2c);
                    
                    trace!(
                        user = %user_s2c,
                        bytes = n,
                        total = total_bytes,
                        "S->C data"
                    );
                    
                    let elapsed = last_log.elapsed();
                    if elapsed > Duration::from_secs(10) {
                        let delta = total_bytes - prev_total_bytes;
                        let rate = delta as f64 / elapsed.as_secs_f64();
                        
                        // Changed to DEBUG to reduce log spam
                        debug!(
                            user = %user_s2c,
                            total_bytes = total_bytes,
                            msgs = msg_count,
                            rate_kbps = (rate / 1024.0) as u64,
                            "S->C transfer in progress"
                        );
                        
                        last_log = Instant::now();
                        prev_total_bytes = total_bytes;
                    }
                    
                    if let Err(e) = client_writer.write_all(&buf[..n]).await {
                        debug!(user = %user_s2c, error = %e, "Failed to write to client");
                        break;
                    }
                    if let Err(e) = client_writer.flush().await {
                        debug!(user = %user_s2c, error = %e, "Failed to flush to client");
                        break;
                    }
                }
                
                Ok(Err(e)) => {
                    debug!(user = %user_s2c, error = %e, total_bytes = total_bytes, "Server read error");
                    break;
                }
            }
        }
    });
    
    // Wait for either direction to complete
    tokio::select! {
        result = c2s => {
            if let Err(e) = result {
                warn!(error = %e, "C->S task panicked");
            }
        }
        result = s2c => {
            if let Err(e) = result {
                warn!(error = %e, "S->C task panicked");
            }
        }
    }
    
    debug!(
        c2s_bytes = c2s_bytes.load(Ordering::Relaxed),
        s2c_bytes = s2c_bytes.load(Ordering::Relaxed),
        "Relay finished"
    );
    
    Ok(())
}