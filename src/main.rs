//! Telemt - MTProxy on Rust

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::signal;
use tracing::{info, error, warn};
use tracing_subscriber::{fmt, EnvFilter};

mod config;
mod crypto;
mod error;
mod protocol;
mod proxy;
mod stats;
mod stream;
mod transport;
mod util;

use crate::config::ProxyConfig;
use crate::proxy::ClientHandler;
use crate::stats::Stats;
use crate::transport::{create_listener, ListenOptions, UpstreamManager};
use crate::util::ip::detect_ip;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    // Load config
    let config_path = std::env::args().nth(1).unwrap_or_else(|| "config.toml".to_string());
    let config = match ProxyConfig::load(&config_path) {
        Ok(c) => c,
        Err(e) => {
            // If config doesn't exist, try to create default
            if std::path::Path::new(&config_path).exists() {
                error!("Failed to load config: {}", e);
                std::process::exit(1);
            } else {
                let default = ProxyConfig::default();
                let toml = toml::to_string_pretty(&default).unwrap();
                std::fs::write(&config_path, toml).unwrap();
                info!("Created default config at {}", config_path);
                default
            }
        }
    };
    
    config.validate()?;
    
    let config = Arc::new(config);
    let stats = Arc::new(Stats::new());
    
    // Initialize Upstream Manager
    let upstream_manager = Arc::new(UpstreamManager::new(config.upstreams.clone()));
    
    // Start Health Checks
    let um_clone = upstream_manager.clone();
    tokio::spawn(async move {
        um_clone.run_health_checks().await;
    });

    // Detect public IP if needed (once at startup)
    let detected_ip = detect_ip().await;

    // Start Listeners
    let mut listeners = Vec::new();
    
    for listener_conf in &config.listeners {
        let addr = SocketAddr::new(listener_conf.ip, config.port);
        let options = ListenOptions {
            ipv6_only: listener_conf.ip.is_ipv6(),
            ..Default::default()
        };
        
        match create_listener(addr, &options) {
            Ok(socket) => {
                let listener = TcpListener::from_std(socket.into())?;
                info!("Listening on {}", addr);
                
                // Determine public IP for tg:// links
                // 1. Use explicit announce_ip if set
                // 2. If listening on 0.0.0.0 or ::, use detected public IP
                // 3. Otherwise use the bind IP
                let public_ip = if let Some(ip) = listener_conf.announce_ip {
                    ip
                } else if listener_conf.ip.is_unspecified() {
                    // Try to use detected IP of the same family
                    if listener_conf.ip.is_ipv4() {
                        detected_ip.ipv4.unwrap_or(listener_conf.ip)
                    } else {
                        detected_ip.ipv6.unwrap_or(listener_conf.ip)
                    }
                } else {
                    listener_conf.ip
                };

                // Show links for configured users
                if !config.show_link.is_empty() {
                    info!("--- Proxy Links for {} ---", public_ip);
                    for user_name in &config.show_link {
                        if let Some(secret) = config.users.get(user_name) {
                            info!("User: {}", user_name);

                            // Classic
                            if config.modes.classic {
                                info!("  Classic: tg://proxy?server={}&port={}&secret={}", 
                                    public_ip, config.port, secret);
                            }

                            // DD (Secure)
                            if config.modes.secure {
                                info!("  DD:      tg://proxy?server={}&port={}&secret=dd{}", 
                                    public_ip, config.port, secret);
                            }

                            // EE-TLS (FakeTLS)
                            if config.modes.tls {
                                let domain_hex = hex::encode(&config.tls_domain);
                                info!("  EE-TLS:  tg://proxy?server={}&port={}&secret=ee{}{}", 
                                    public_ip, config.port, secret, domain_hex);
                            }
                        } else {
                            warn!("User '{}' specified in show_link not found in users list", user_name);
                        }
                    }
                    info!("-----------------------------------");
                }
                
                listeners.push(listener);
            },
            Err(e) => {
                error!("Failed to bind to {}: {}", addr, e);
            }
        }
    }
    
    if listeners.is_empty() {
        error!("No listeners could be started. Exiting.");
        std::process::exit(1);
    }

    // Accept loop
    // For simplicity in this slice, we just spawn a task for each listener
    // In a real high-perf scenario, we might want a more complex accept loop
    
    for listener in listeners {
        let config = config.clone();
        let stats = stats.clone();
        let upstream_manager = upstream_manager.clone();
        
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer_addr)) => {
                        let config = config.clone();
                        let stats = stats.clone();
                        let upstream_manager = upstream_manager.clone();
                        
                        tokio::spawn(async move {
                            if let Err(e) = ClientHandler::new(
                                stream, 
                                peer_addr, 
                                config, 
                                stats,
                                upstream_manager
                            ).run().await {
                                // Log only relevant errors
                                // debug!("Connection error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("Accept error: {}", e);
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        });
    }

    // Wait for signal
    match signal::ctrl_c().await {
        Ok(()) => info!("Shutting down..."),
        Err(e) => error!("Signal error: {}", e),
    }

    Ok(())
}