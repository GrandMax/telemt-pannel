//! Client Handler

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::io::{AsyncRead, AsyncWrite, AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;
use tracing::{debug, info, warn, error, trace};

use crate::config::ProxyConfig;
use crate::error::{ProxyError, Result, HandshakeResult};
use crate::protocol::constants::*;
use crate::protocol::tls;
use crate::stats::{Stats, ReplayChecker};
use crate::transport::{configure_client_socket, UpstreamManager};
use crate::stream::{CryptoReader, CryptoWriter, FakeTlsReader, FakeTlsWriter, BufferPool};
use crate::crypto::AesCtr;

use super::handshake::{
    handle_tls_handshake, handle_mtproto_handshake, 
    HandshakeSuccess, generate_tg_nonce, encrypt_tg_nonce,
};
use super::relay::relay_bidirectional;
use super::masking::handle_bad_client;

/// Client connection handler (builder struct)
pub struct ClientHandler;

/// Running client handler with stream and context
pub struct RunningClientHandler {
    stream: TcpStream,
    peer: SocketAddr,
    config: Arc<ProxyConfig>,
    stats: Arc<Stats>,
    replay_checker: Arc<ReplayChecker>,
    upstream_manager: Arc<UpstreamManager>,
    buffer_pool: Arc<BufferPool>,
}

impl ClientHandler {
    /// Create new client handler instance
    pub fn new(
        stream: TcpStream,
        peer: SocketAddr,
        config: Arc<ProxyConfig>,
        stats: Arc<Stats>,
        upstream_manager: Arc<UpstreamManager>,
        replay_checker: Arc<ReplayChecker>,
        buffer_pool: Arc<BufferPool>,
    ) -> RunningClientHandler {
        RunningClientHandler {
            stream,
            peer,
            config,
            stats,
            replay_checker,
            upstream_manager,
            buffer_pool,
        }
    }
}

impl RunningClientHandler {
    /// Run the client handler
    pub async fn run(mut self) -> Result<()> {
        self.stats.increment_connects_all();
        
        let peer = self.peer;
        debug!(peer = %peer, "New connection");
        
        // Configure socket
        if let Err(e) = configure_client_socket(
            &self.stream,
            self.config.timeouts.client_keepalive,
            self.config.timeouts.client_ack,
        ) {
            debug!(peer = %peer, error = %e, "Failed to configure client socket");
        }
        
        // Perform handshake with timeout
        let handshake_timeout = Duration::from_secs(self.config.timeouts.client_handshake);
        
        // Clone stats for error handling block
        let stats = self.stats.clone();
        
        let result = timeout(
            handshake_timeout,
            self.do_handshake()
        ).await;
        
        match result {
            Ok(Ok(())) => {
                debug!(peer = %peer, "Connection handled successfully");
                Ok(())
            }
            Ok(Err(e)) => {
                debug!(peer = %peer, error = %e, "Handshake failed");
                Err(e)
            }
            Err(_) => {
                stats.increment_handshake_timeouts();
                debug!(peer = %peer, "Handshake timeout");
                Err(ProxyError::TgHandshakeTimeout)
            }
        }
    }
    
    /// Perform handshake and relay
    async fn do_handshake(mut self) -> Result<()> {
        // Read first bytes to determine handshake type
        let mut first_bytes = [0u8; 5];
        self.stream.read_exact(&mut first_bytes).await?;
        
        let is_tls = tls::is_tls_handshake(&first_bytes[..3]);
        let peer = self.peer;
        
        debug!(peer = %peer, is_tls = is_tls, first_bytes = %hex::encode(&first_bytes), "Handshake type detected");
        
        if is_tls {
            self.handle_tls_client(first_bytes).await
        } else {
            self.handle_direct_client(first_bytes).await
        }
    }
    
    /// Handle TLS-wrapped client
    async fn handle_tls_client(
        mut self,
        first_bytes: [u8; 5],
    ) -> Result<()> {
        let peer = self.peer;
        
        // Read TLS handshake length
        let tls_len = u16::from_be_bytes([first_bytes[3], first_bytes[4]]) as usize;
        
        debug!(peer = %peer, tls_len = tls_len, "Reading TLS handshake");
        
        if tls_len < 512 {
            debug!(peer = %peer, tls_len = tls_len, "TLS handshake too short");
            self.stats.increment_connects_bad();
            // FIX: Split stream into reader/writer for handle_bad_client
            let (reader, writer) = self.stream.into_split();
            handle_bad_client(reader, writer, &first_bytes, &self.config).await;
            return Ok(());
        }
        
        // Read full TLS handshake
        let mut handshake = vec![0u8; 5 + tls_len];
        handshake[..5].copy_from_slice(&first_bytes);
        self.stream.read_exact(&mut handshake[5..]).await?;
        
        // Extract fields before consuming self.stream
        let config = self.config.clone();
        let replay_checker = self.replay_checker.clone();
        let stats = self.stats.clone();
        let buffer_pool = self.buffer_pool.clone();
        
        // Split stream for reading/writing
        let (read_half, write_half) = self.stream.into_split();
        
        // Handle TLS handshake
        let (mut tls_reader, tls_writer, _tls_user) = match handle_tls_handshake(
            &handshake,
            read_half,
            write_half,
            peer,
            &config,
            &replay_checker,
        ).await {
            HandshakeResult::Success(result) => result,
            HandshakeResult::BadClient { reader, writer } => {
                stats.increment_connects_bad();
                handle_bad_client(reader, writer, &handshake, &config).await;
                return Ok(());
            }
            HandshakeResult::Error(e) => return Err(e),
        };
        
        // Read MTProto handshake through TLS
        debug!(peer = %peer, "Reading MTProto handshake through TLS");
        let mtproto_data = tls_reader.read_exact(HANDSHAKE_LEN).await?;
        let mtproto_handshake: [u8; HANDSHAKE_LEN] = mtproto_data[..].try_into()
            .map_err(|_| ProxyError::InvalidHandshake("Short MTProto handshake".into()))?;
        
        // Handle MTProto handshake
        let (crypto_reader, crypto_writer, success) = match handle_mtproto_handshake(
            &mtproto_handshake,
            tls_reader,
            tls_writer,
            peer,
            &config,
            &replay_checker,
            true,
        ).await {
            HandshakeResult::Success(result) => result,
            HandshakeResult::BadClient { reader, writer } => {
                stats.increment_connects_bad();
                // Valid TLS but invalid MTProto - drop
                debug!(peer = %peer, "Valid TLS but invalid MTProto handshake - dropping");
                return Ok(());
            }
            HandshakeResult::Error(e) => return Err(e),
        };
        
        Self::handle_authenticated_static(
            crypto_reader, 
            crypto_writer, 
            success, 
            self.upstream_manager, 
            self.stats, 
            self.config,
            buffer_pool
        ).await
    }
    
    /// Handle direct (non-TLS) client
    async fn handle_direct_client(
        mut self,
        first_bytes: [u8; 5],
    ) -> Result<()> {
        let peer = self.peer;
        
        // Check if non-TLS modes are enabled
        if !self.config.general.modes.classic && !self.config.general.modes.secure {
            debug!(peer = %peer, "Non-TLS modes disabled");
            self.stats.increment_connects_bad();
            // FIX: Split stream into reader/writer for handle_bad_client
            let (reader, writer) = self.stream.into_split();
            handle_bad_client(reader, writer, &first_bytes, &self.config).await;
            return Ok(());
        }
        
        // Read rest of handshake
        let mut handshake = [0u8; HANDSHAKE_LEN];
        handshake[..5].copy_from_slice(&first_bytes);
        self.stream.read_exact(&mut handshake[5..]).await?;
        
        // Extract fields
        let config = self.config.clone();
        let replay_checker = self.replay_checker.clone();
        let stats = self.stats.clone();
        let buffer_pool = self.buffer_pool.clone();
        
        // Split stream
        let (read_half, write_half) = self.stream.into_split();
        
        // Handle MTProto handshake
        let (crypto_reader, crypto_writer, success) = match handle_mtproto_handshake(
            &handshake,
            read_half,
            write_half,
            peer,
            &config,
            &replay_checker,
            false,
        ).await {
            HandshakeResult::Success(result) => result,
            HandshakeResult::BadClient { reader, writer } => {
                stats.increment_connects_bad();
                handle_bad_client(reader, writer, &handshake, &config).await;
                return Ok(());
            }
            HandshakeResult::Error(e) => return Err(e),
        };
        
        Self::handle_authenticated_static(
            crypto_reader, 
            crypto_writer, 
            success, 
            self.upstream_manager, 
            self.stats, 
            self.config,
            buffer_pool
        ).await
    }
    
    /// Static version of handle_authenticated_inner
    async fn handle_authenticated_static<R, W>(
        client_reader: CryptoReader<R>,
        client_writer: CryptoWriter<W>,
        success: HandshakeSuccess,
        upstream_manager: Arc<UpstreamManager>,
        stats: Arc<Stats>,
        config: Arc<ProxyConfig>,
        buffer_pool: Arc<BufferPool>,
    ) -> Result<()>
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let user = &success.user;
        
        // Check user limits
        if let Err(e) = Self::check_user_limits_static(user, &config, &stats) {
            warn!(user = %user, error = %e, "User limit exceeded");
            return Err(e);
        }
        
        // Get datacenter address
        let dc_addr = Self::get_dc_addr_static(success.dc_idx, &config)?;
        
        info!(
            user = %user,
            peer = %success.peer,
            dc = success.dc_idx,
            dc_addr = %dc_addr,
            proto = ?success.proto_tag,
            fast_mode = config.general.fast_mode,
            "Connecting to Telegram"
        );
        
        // Connect to Telegram via UpstreamManager
        let tg_stream = upstream_manager.connect(dc_addr).await?;
        
        debug!(peer = %success.peer, dc_addr = %dc_addr, "Connected to Telegram, performing handshake");
        
        // Perform Telegram handshake and get crypto streams
        let (tg_reader, tg_writer) = Self::do_tg_handshake_static(
            tg_stream, 
            &success,
            &config,
        ).await?;
        
        debug!(peer = %success.peer, "Telegram handshake complete, starting relay");
        
        // Update stats
        stats.increment_user_connects(user);
        stats.increment_user_curr_connects(user);
        
        // Relay traffic using buffer pool
        let relay_result = relay_bidirectional(
            client_reader,
            client_writer,
            tg_reader,
            tg_writer,
            user,
            Arc::clone(&stats),
            buffer_pool,
        ).await;
        
        // Update stats
        stats.decrement_user_curr_connects(user);
        
        match &relay_result {
            Ok(()) => debug!(user = %user, peer = %success.peer, "Relay completed normally"),
            Err(e) => debug!(user = %user, peer = %success.peer, error = %e, "Relay ended with error"),
        }
        
        relay_result
    }
    
    /// Check user limits (static version)
    fn check_user_limits_static(user: &str, config: &ProxyConfig, stats: &Stats) -> Result<()> {
        // Check expiration
        if let Some(expiration) = config.access.user_expirations.get(user) {
            if chrono::Utc::now() > *expiration {
                return Err(ProxyError::UserExpired { user: user.to_string() });
            }
        }
        
        // Check connection limit
        if let Some(limit) = config.access.user_max_tcp_conns.get(user) {
            let current = stats.get_user_curr_connects(user);
            if current >= *limit as u64 {
                return Err(ProxyError::ConnectionLimitExceeded { user: user.to_string() });
            }
        }
        
        // Check data quota
        if let Some(quota) = config.access.user_data_quota.get(user) {
            let used = stats.get_user_total_octets(user);
            if used >= *quota {
                return Err(ProxyError::DataQuotaExceeded { user: user.to_string() });
            }
        }
        
        Ok(())
    }
    
    /// Get datacenter address by index (static version)
    fn get_dc_addr_static(dc_idx: i16, config: &ProxyConfig) -> Result<SocketAddr> {
        let idx = (dc_idx.abs() - 1) as usize;
        
        let datacenters = if config.general.prefer_ipv6 {
            &*TG_DATACENTERS_V6
        } else {
            &*TG_DATACENTERS_V4
        };
        
        datacenters.get(idx)
            .map(|ip| SocketAddr::new(*ip, TG_DATACENTER_PORT))
            .ok_or_else(|| ProxyError::InvalidHandshake(
                format!("Invalid DC index: {}", dc_idx)
            ))
    }
    
    /// Perform handshake with Telegram server (static version)
    async fn do_tg_handshake_static(
        mut stream: TcpStream,
        success: &HandshakeSuccess,
        config: &ProxyConfig,
    ) -> Result<(CryptoReader<tokio::net::tcp::OwnedReadHalf>, CryptoWriter<tokio::net::tcp::OwnedWriteHalf>)> {
        // Generate nonce with keys for TG
        let (nonce, tg_enc_key, tg_enc_iv, tg_dec_key, tg_dec_iv) = generate_tg_nonce(
            success.proto_tag,
            &success.dec_key,  // Client's dec key
            success.dec_iv,
            config.general.fast_mode,
        );
        
        // Encrypt nonce
        let encrypted_nonce = encrypt_tg_nonce(&nonce);
        
        debug!(
            peer = %success.peer,
            nonce_head = %hex::encode(&nonce[..16]),
            encrypted_head = %hex::encode(&encrypted_nonce[..16]),
            "Sending nonce to Telegram"
        );
        
        // Send to Telegram
        stream.write_all(&encrypted_nonce).await?;
        stream.flush().await?;
        
        debug!(peer = %success.peer, "Nonce sent to Telegram");
        
        // Split stream and wrap with crypto
        let (read_half, write_half) = stream.into_split();
        
        let decryptor = AesCtr::new(&tg_dec_key, tg_dec_iv);
        let encryptor = AesCtr::new(&tg_enc_key, tg_enc_iv);
        
        let tg_reader = CryptoReader::new(read_half, decryptor);
        let tg_writer = CryptoWriter::new(write_half, encryptor);
        
        Ok((tg_reader, tg_writer))
    }
}