//! Masking - forward unrecognized traffic to mask host

use std::time::Duration;
use std::str;
use tokio::net::TcpStream;
use tokio::io::{AsyncRead, AsyncWrite, AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;
use tracing::debug;
use crate::config::ProxyConfig;

const MASK_TIMEOUT: Duration = Duration::from_secs(5);
const MASK_BUFFER_SIZE: usize = 8192;

/// Detect client type based on initial data
fn detect_client_type(data: &[u8]) -> &'static str {
    // Check for HTTP request
    if data.len() > 4 {
        if data.starts_with(b"GET ") || data.starts_with(b"POST") ||
           data.starts_with(b"HEAD") || data.starts_with(b"PUT ") ||
           data.starts_with(b"DELETE") || data.starts_with(b"OPTIONS") {
            return "HTTP";
        }
    }
    
    // Check for TLS ClientHello (0x16 = handshake, 0x03 0x01-0x03 = TLS version)
    if data.len() > 3 && data[0] == 0x16 && data[1] == 0x03 {
        return "TLS-scanner";
    }
    
    // Check for SSH
    if data.starts_with(b"SSH-") {
        return "SSH";
    }
    
    // Port scanner (very short data)
    if data.len() < 10 {
        return "port-scanner";
    }
    
    "unknown"
}

/// Handle a bad client by forwarding to mask host
pub async fn handle_bad_client<R, W>(
    mut reader: R,
    mut writer: W,
    initial_data: &[u8],
    config: &ProxyConfig,
) 
where 
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    if !config.censorship.mask {
        // Masking disabled, just consume data
        consume_client_data(reader).await;
        return;
    }
    
    let client_type = detect_client_type(initial_data);
    
    let mask_host = config.censorship.mask_host.as_deref()
        .unwrap_or(&config.censorship.tls_domain);
    let mask_port = config.censorship.mask_port;
    
    debug!(
        client_type = client_type,
        host = %mask_host,
        port = mask_port,
        data_len = initial_data.len(),
        "Forwarding bad client to mask host"
    );
    
    // Connect to mask host
    let mask_addr = format!("{}:{}", mask_host, mask_port);
    let connect_result = timeout(
        MASK_TIMEOUT,
        TcpStream::connect(&mask_addr)
    ).await;
    
    let mask_stream = match connect_result {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            debug!(error = %e, "Failed to connect to mask host");
            consume_client_data(reader).await;
            return;
        }
        Err(_) => {
            debug!("Timeout connecting to mask host");
            consume_client_data(reader).await;
            return;
        }
    };
    
    let (mut mask_read, mut mask_write) = mask_stream.into_split();
    
    // Send initial data to mask host
    if mask_write.write_all(initial_data).await.is_err() {
        return;
    }
    
    // Relay traffic
    let c2m = tokio::spawn(async move {
        let mut buf = vec![0u8; MASK_BUFFER_SIZE];
        loop {
            match reader.read(&mut buf).await {
                Ok(0) | Err(_) => {
                    let _ = mask_write.shutdown().await;
                    break;
                }
                Ok(n) => {
                    if mask_write.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
            }
        }
    });
    
    let m2c = tokio::spawn(async move {
        let mut buf = vec![0u8; MASK_BUFFER_SIZE];
        loop {
            match mask_read.read(&mut buf).await {
                Ok(0) | Err(_) => {
                    let _ = writer.shutdown().await;
                    break;
                }
                Ok(n) => {
                    if writer.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
            }
        }
    });
    
    // Wait for either to complete
    tokio::select! {
        _ = c2m => {}
        _ = m2c => {}
    }
}

/// Just consume all data from client without responding
async fn consume_client_data<R: AsyncRead + Unpin>(mut reader: R) {
    let mut buf = vec![0u8; MASK_BUFFER_SIZE];
    while let Ok(n) = reader.read(&mut buf).await {
        if n == 0 {
            break;
        }
    }
}