//! Middle Proxy RPC Transport
    //!
    //! Implements Telegram Middle-End RPC protocol for routing to ALL DCs (including CDN).
    //! Uses existing crypto primitives from crate::crypto.
    
    use std::collections::HashMap;
    use std::net::{IpAddr, SocketAddr};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;
    use bytes::{Bytes, BytesMut};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;
    use tokio::sync::{mpsc, Mutex, RwLock};
    use tokio::time::{timeout, Instant};
    use tracing::{debug, info, trace, warn};
    
    use crate::crypto::{sha1, crc32, derive_middleproxy_keys, AesCbc, SecureRandom};
    use crate::error::{ProxyError, Result};
    use crate::protocol::constants::*;
    
    // ========== RPC Nonce (32 bytes) ==========
    
    fn build_nonce_packet(key_selector: u32, crypto_ts: u32, nonce: &[u8; 16]) -> [u8; 32] {
        let mut p = [0u8; 32];
        p[0..4].copy_from_slice(&RPC_NONCE_U32.to_le_bytes());
        p[4..8].copy_from_slice(&key_selector.to_le_bytes());
        p[8..12].copy_from_slice(&RPC_CRYPTO_AES_U32.to_le_bytes());
        p[12..16].copy_from_slice(&crypto_ts.to_le_bytes());
        p[16..32].copy_from_slice(nonce);
        p
    }
    
    fn parse_nonce_response(d: &[u8; 32]) -> Result<(u32, u32, [u8; 16])> {
        let t = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
        if t != RPC_NONCE_U32 {
            return Err(ProxyError::InvalidHandshake(format!("Expected RPC_NONCE, got 0x{:08x}", t)));
        }
        let schema = u32::from_le_bytes([d[8], d[9], d[10], d[11]]);
        let ts = u32::from_le_bytes([d[12], d[13], d[14], d[15]]);
        let mut nonce = [0u8; 16];
        nonce.copy_from_slice(&d[16..32]);
        Ok((schema, ts, nonce))
    }
    
    // ========== RPC Handshake (32 bytes) ==========
    
    fn build_handshake_packet() -> [u8; 32] {
        let mut p = [0u8; 32];
        p[0..4].copy_from_slice(&RPC_HANDSHAKE_U32.to_le_bytes());
        // flags=0, sender_pid with our PID
        let pid = (std::process::id() & 0xFFFF) as u16;
        p[14..16].copy_from_slice(&pid.to_le_bytes());
        let utime = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default().as_secs() as u32;
        p[16..20].copy_from_slice(&utime.to_le_bytes());
        p
    }
    
    // ========== CRC32 RPC Frame ==========
    
    fn build_rpc_frame(seq_no: i32, payload: &[u8]) -> Vec<u8> {
        let total = (4 + 4 + payload.len() + 4) as u32;
        let mut f = Vec::with_capacity(total as usize);
        f.extend_from_slice(&total.to_le_bytes());
        f.extend_from_slice(&seq_no.to_le_bytes());
        f.extend_from_slice(payload);
        let c = crc32(&f);
        f.extend_from_slice(&c.to_le_bytes());
        f
    }
    
    // ========== RPC_PROXY_REQ ==========
    
    fn build_proxy_req_payload(
        conn_id: u64,
        client_addr: SocketAddr,
        our_addr: SocketAddr,
        data: &[u8],
        proxy_tag: Option<&[u8]>,
    ) -> Vec<u8> {
        let mut flags: u32 = proxy_flags::FLAG_HAS_AD_TAG2 | proxy_flags::FLAG_EXTMODE2;
        if proxy_tag.is_some() {
            flags |= proxy_flags::FLAG_HAS_AD_TAG;
        }
    
        let extra_words: u32 = if let Some(tag) = proxy_tag {
            let tl_len = 1 + tag.len();
            let padded = (tl_len + 3) / 4;
            (1 + padded) as u32
        } else { 0 };
    
        let mut b = Vec::with_capacity(64 + data.len());
        b.extend_from_slice(&RPC_PROXY_REQ_U32.to_le_bytes());
        b.extend_from_slice(&flags.to_le_bytes());
        b.extend_from_slice(&conn_id.to_le_bytes());
    
        // Client IP
        match client_addr.ip() {
            IpAddr::V4(v4) => b.extend_from_slice(&u32::from_be_bytes(v4.octets()).to_le_bytes()),
            IpAddr::V6(_) => b.extend_from_slice(&0u32.to_le_bytes()),
        }
        b.extend_from_slice(&(client_addr.port() as u32).to_le_bytes());
        // Our IP
        match our_addr.ip() {
            IpAddr::V4(v4) => b.extend_from_slice(&u32::from_be_bytes(v4.octets()).to_le_bytes()),
            IpAddr::V6(_) => b.extend_from_slice(&0u32.to_le_bytes()),
        }
        b.extend_from_slice(&(our_addr.port() as u32).to_le_bytes());
        b.extend_from_slice(&extra_words.to_le_bytes());
    
        if let Some(tag) = proxy_tag {
            b.extend_from_slice(&TL_PROXY_TAG_U32.to_le_bytes());
            b.push(tag.len() as u8);
            b.extend_from_slice(tag);
            let pad = (4 - ((1 + tag.len()) % 4)) % 4;
            b.extend(std::iter::repeat(0u8).take(pad));
        }
    
        b.extend_from_slice(data);
        b
    }
    
    // ========== ME Response ==========
    
    #[derive(Debug)]
    pub enum MeResponse {
        Data(Bytes),
        Ack(u32),
        Close,
    }
    
    // ========== Connection Registry ==========
    
    pub struct ConnRegistry {
        map: RwLock<HashMap<u64, mpsc::Sender<MeResponse>>>,
        next_id: AtomicU64,
    }
    
    impl ConnRegistry {
        pub fn new() -> Self {
            Self { map: RwLock::new(HashMap::new()), next_id: AtomicU64::new(1) }
        }
        pub async fn register(&self) -> (u64, mpsc::Receiver<MeResponse>) {
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            let (tx, rx) = mpsc::channel(256);
            self.map.write().await.insert(id, tx);
            (id, rx)
        }
        pub async fn unregister(&self, id: u64) {
            self.map.write().await.remove(&id);
        }
        pub async fn route(&self, id: u64, resp: MeResponse) -> bool {
            let m = self.map.read().await;
            if let Some(tx) = m.get(&id) { tx.send(resp).await.is_ok() } else { false }
        }
    }
    
    // ========== RPC Writer (streaming CBC) ==========
    
    struct RpcWriter {
        writer: tokio::io::WriteHalf<TcpStream>,
        key: [u8; 32],
        iv: [u8; 16],
        seq_no: i32,
    }
    
    impl RpcWriter {
        async fn send(&mut self, payload: &[u8]) -> Result<()> {
            let frame = build_rpc_frame(self.seq_no, payload);
            self.seq_no += 1;
            let pad = (16 - (frame.len() % 16)) % 16;
            let mut buf = frame;
            buf.extend(std::iter::repeat(0u8).take(pad));
    
            let cipher = AesCbc::new(self.key, self.iv);
            cipher.encrypt_in_place(&mut buf)
                .map_err(|e| ProxyError::Crypto(format!("{}", e)))?;
            if buf.len() >= 16 {
                self.iv.copy_from_slice(&buf[buf.len() - 16..]);
            }
            self.writer.write_all(&buf).await.map_err(ProxyError::Io)
        }
    }
    
    // ========== ME Pool ==========
    
    pub struct MePool {
        registry: Arc<ConnRegistry>,
        writers: RwLock<Vec<Arc<Mutex<RpcWriter>>>>,
        rr: AtomicU64,
        proxy_tag: Option<Vec<u8>>,
    }
    
    impl MePool {
        pub fn new(proxy_tag: Option<Vec<u8>>) -> Arc<Self> {
            Arc::new(Self {
                registry: Arc::new(ConnRegistry::new()),
                writers: RwLock::new(Vec::new()),
                rr: AtomicU64::new(0),
                proxy_tag,
            })
        }
        pub fn registry(&self) -> &Arc<ConnRegistry> { &self.registry }
    
        pub async fn init(self: &Arc<Self>, pool_size: usize, secret: &[u8], rng: &SecureRandom) -> Result<()> {
            let addrs = &*TG_MIDDLE_PROXIES_FLAT_V4;
            info!(me_servers = addrs.len(), pool_size, "Initializing ME pool");
            for &(ip, port) in addrs.iter().take(3) {
                for i in 0..pool_size {
                    let addr = SocketAddr::new(ip, port);
                    match self.connect(addr, secret, rng).await {
                        Ok(()) => info!(%addr, idx = i, "ME connected"),
                        Err(e) => warn!(%addr, idx = i, error = %e, "ME connect failed"),
                    }
                }
            }
            if self.writers.read().await.is_empty() {
                return Err(ProxyError::Proxy("No ME connections".into()));
            }
            Ok(())
        }
    
        async fn connect(self: &Arc<Self>, addr: SocketAddr, secret: &[u8], rng: &SecureRandom) -> Result<()> {
            let stream = timeout(Duration::from_secs(ME_CONNECT_TIMEOUT_SECS), TcpStream::connect(addr))
                .await.map_err(|_| ProxyError::ConnectionTimeout { addr: addr.to_string() })?
                .map_err(ProxyError::Io)?;
            stream.set_nodelay(true).ok();
            let (mut rd, mut wr) = tokio::io::split(stream);
    
            // Nonce exchange
            let my_nonce: [u8; 16] = rng.bytes(16).try_into().unwrap();
            let crypto_ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as u32;
            let sh = sha1(secret);
            let ks = u32::from_le_bytes([sh[0], sh[1], sh[2], sh[3]]);
            wr.write_all(&build_nonce_packet(ks, crypto_ts, &my_nonce)).await.map_err(ProxyError::Io)?;
    
            let mut resp = [0u8; 32];
            timeout(Duration::from_secs(ME_HANDSHAKE_TIMEOUT_SECS), rd.read_exact(&mut resp))
                .await.map_err(|_| ProxyError::TgHandshakeTimeout)?.map_err(ProxyError::Io)?;
            let (schema, _srv_ts, srv_nonce) = parse_nonce_response(&resp)?;
            if schema != RPC_CRYPTO_AES_U32 {
                return Err(ProxyError::InvalidHandshake(format!("Unsupported crypto: {}", schema)));
            }
    
            // Key derivation via existing derive_middleproxy_keys
            let ts_bytes = crypto_ts.to_le_bytes();
            let (wk, wi) = derive_middleproxy_keys(
                &srv_nonce, &my_nonce, &ts_bytes,
                None, &0u16.to_le_bytes(), b"CLIENT", None, &0u16.to_le_bytes(), secret, None, None,
            );
            let (rk, ri) = derive_middleproxy_keys(
                &srv_nonce, &my_nonce, &ts_bytes,
                None, &0u16.to_le_bytes(), b"SERVER", None, &0u16.to_le_bytes(), secret, None, None,
            );
    
            // Handshake
            wr.write_all(&build_handshake_packet()).await.map_err(ProxyError::Io)?;
            let mut hs = [0u8; 32];
            timeout(Duration::from_secs(ME_HANDSHAKE_TIMEOUT_SECS), rd.read_exact(&mut hs))
                .await.map_err(|_| ProxyError::TgHandshakeTimeout)?.map_err(ProxyError::Io)?;
            let ht = u32::from_le_bytes([hs[0], hs[1], hs[2], hs[3]]);
            if ht == RPC_HANDSHAKE_ERROR_U32 {
                return Err(ProxyError::InvalidHandshake("ME rejected handshake".into()));
            }
            if ht != RPC_HANDSHAKE_U32 {
                return Err(ProxyError::InvalidHandshake(format!("Got 0x{:08x}", ht)));
            }
    
            info!(%addr, "RPC handshake OK");
    
            self.writers.write().await.push(Arc::new(Mutex::new(RpcWriter { writer: wr, key: wk, iv: wi, seq_no: 0 })));
    
            let reg = self.registry.clone();
            tokio::spawn(async move {
                if let Err(e) = reader_loop(rd, rk, ri, reg).await {
                    warn!(error = %e, "ME reader ended");
                }
            });
            Ok(())
        }
    
        pub async fn send_proxy_req(&self, conn_id: u64, client_addr: SocketAddr, our_addr: SocketAddr, data: &[u8]) -> Result<()> {
            let ws = self.writers.read().await;
            if ws.is_empty() { return Err(ProxyError::Proxy("No ME connections".into())); }
            let w = ws[self.rr.fetch_add(1, Ordering::Relaxed) as usize % ws.len()].clone();
            drop(ws);
            let payload = build_proxy_req_payload(conn_id, client_addr, our_addr, data, self.proxy_tag.as_deref());
            w.lock().await.send(&payload).await
        }
    
        pub async fn send_close(&self, conn_id: u64) -> Result<()> {
            let ws = self.writers.read().await;
            if !ws.is_empty() {
                let w = ws[0].clone();
                drop(ws);
                let mut p = Vec::with_capacity(12);
                p.extend_from_slice(&RPC_CLOSE_EXT_U32.to_le_bytes());
                p.extend_from_slice(&conn_id.to_le_bytes());
                let _ = w.lock().await.send(&p).await;
            }
            self.registry.unregister(conn_id).await;
            Ok(())
        }
    }
    
    // ========== Reader Loop ==========
    
    async fn reader_loop(
        mut rd: tokio::io::ReadHalf<TcpStream>,
        dk: [u8; 32], mut div: [u8; 16],
        reg: Arc<ConnRegistry>,
    ) -> Result<()> {
        let mut raw = BytesMut::with_capacity(65536);
        let mut dec = BytesMut::new();
        loop {
            let mut tmp = [0u8; 16384];
            let n = rd.read(&mut tmp).await.map_err(ProxyError::Io)?;
            if n == 0 { return Ok(()); }
            raw.extend_from_slice(&tmp[..n]);
    
            let blocks = raw.len() / 16 * 16;
            if blocks > 0 {
                let mut new_iv = [0u8; 16];
                new_iv.copy_from_slice(&raw[blocks - 16..blocks]);
                let mut chunk = vec![0u8; blocks];
                chunk.copy_from_slice(&raw[..blocks]);
                AesCbc::new(dk, div).decrypt_in_place(&mut chunk)
                    .map_err(|e| ProxyError::Crypto(format!("{}", e)))?;
                div = new_iv;
                dec.extend_from_slice(&chunk);
                let _ = raw.split_to(blocks);
            }
    
            while dec.len() >= 12 {
                let fl = u32::from_le_bytes([dec[0], dec[1], dec[2], dec[3]]) as usize;
                if fl == 4 { let _ = dec.split_to(4); continue; }
                if fl < 12 || fl > (1 << 24) { dec.clear(); break; }
                if dec.len() < fl { break; }
                let frame = dec.split_to(fl);
                // CRC32 check
                let pe = fl - 4;
                let ec = u32::from_le_bytes([frame[pe], frame[pe+1], frame[pe+2], frame[pe+3]]);
                if crc32(&frame[..pe]) != ec { warn!("CRC mismatch"); continue; }
                let payload = &frame[8..pe];
                if payload.len() < 4 { continue; }
                let pt = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
                let body = &payload[4..];
    
                if pt == RPC_PROXY_ANS_U32 && body.len() >= 12 {
                    let flags = u32::from_le_bytes(body[0..4].try_into().unwrap());
                    let cid = u64::from_le_bytes(body[4..12].try_into().unwrap());
                    let data = Bytes::copy_from_slice(&body[12..]);
                    trace!(cid, len = data.len(), flags, "ANS");
                    reg.route(cid, MeResponse::Data(data)).await;
                } else if pt == RPC_SIMPLE_ACK_U32 && body.len() >= 12 {
                    let cid = u64::from_le_bytes(body[0..8].try_into().unwrap());
                    let cfm = u32::from_le_bytes(body[8..12].try_into().unwrap());
                    trace!(cid, cfm, "ACK");
                    reg.route(cid, MeResponse::Ack(cfm)).await;
                } else if pt == RPC_CLOSE_EXT_U32 && body.len() >= 8 {
                    let cid = u64::from_le_bytes(body[0..8].try_into().unwrap());
                    debug!(cid, "CLOSE_EXT");
                    reg.route(cid, MeResponse::Close).await;
                    reg.unregister(cid).await;
                }
            }
        }
    }
    