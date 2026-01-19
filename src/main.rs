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
use crate::stats::{Stats, ReplayChecker};
use crate::transport::{create_listener, ListenOptions, UpstreamManager};
use crate::util::ip::detect_ip;
use crate::stream::BufferPool;

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
    
    // Log loaded configuration for debugging
    info!("=== Configuration Loaded ===");
    info!("TLS Domain: {}", config.censorship.tls_domain);
    info!("Mask enabled: {}", config.censorship.mask);
    info!("Mask host: {}", config.censorship.mask_host.as_deref().unwrap_or(&config.censorship.tls_domain));
    info!("Mask port: {}", config.censorship.mask_port);
    info!("Modes: classic={}, secure={}, tls={}", 
        config.general.modes.classic, 
        config.general.modes.secure, 
        config.general.modes.tls
    );
    info!("============================");
    
    let config = Arc::new(config);
    let stats = Arc::new(Stats::new());
    
    // Initialize global ReplayChecker
    // Using sharded implementation for better concurrency
    let replay_checker = Arc::new(ReplayChecker::new(config.access.replay_check_len));
    
    // Initialize Upstream Manager
    let upstream_manager = Arc::new(UpstreamManager::new(config.upstreams.clone()));
    
    // Initialize Buffer Pool
    // 16KB buffers, max 4096 buffers (~64MB total cached)
    let buffer_pool = Arc::new(BufferPool::with_config(16 * 1024, 4096));
    
    // Start Health Checks
    let um_clone = upstream_manager.clone();
    tokio::spawn(async move {
        um_clone.run_health_checks().await;
    });

    // Detect public IP if needed (once at startup)
    let detected_ip = detect_ip().await;

    // Start Listeners
    let mut listeners = Vec::new();
    
    for listener_conf in &config.server.listeners {
        let addr = SocketAddr::new(listener_conf.ip, config.server.port);
        let options = ListenOptions {
            ipv6_only: listener_conf.ip.is_ipv6(),
            ..Default::default()
        };
        
        match create_listener(addr, &options) {
            Ok(socket) => {
                let listener = TcpListener::from_std(socket.into())?;
                info!("Listening on {}", addr);
                
                // Determine public IP for tg:// links
                let public_ip = if let Some(ip) = listener_conf.announce_ip {
                    ip
                } else if listener_conf.ip.is_unspecified() {
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
                        if let Some(secret) = config.access.users.get(user_name) {
                            info!("User: {}", user_name);

                            if config.general.modes.classic {
                                info!("  Classic: tg://proxy?server={}&port={}&secret={}", 
                                    public_ip, config.server.port, secret);
                            }

                            if config.general.modes.secure {
                                info!("  DD:      tg://proxy?server={}&port={}&secret=dd{}", 
                                    public_ip, config.server.port, secret);
                            }

                            if config.general.modes.tls {
                                let domain_hex = hex::encode(&config.censorship.tls_domain);
                                info!("  EE-TLS:  tg://proxy?server={}&port={}&secret=ee{}{}", 
                                    public_ip, config.server.port, secret, domain_hex);
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
    for listener in listeners {
        let config = config.clone();
        let stats = stats.clone();
        let upstream_manager = upstream_manager.clone();
        let replay_checker = replay_checker.clone();
        let buffer_pool = buffer_pool.clone();
        
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer_addr)) => {
                        let config = config.clone();
                        let stats = stats.clone();
                        let upstream_manager = upstream_manager.clone();
                        let replay_checker = replay_checker.clone();
                        let buffer_pool = buffer_pool.clone();
                        
                        tokio::spawn(async move {
                            if let Err(e) = ClientHandler::new(
                                stream, 
                                peer_addr, 
                                config, 
                                stats,
                                upstream_manager,
                                replay_checker,
                                buffer_pool
                            ).run().await {
                                // Log only relevant errors
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