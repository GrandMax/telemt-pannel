//! MTProto Handshake Magics

use std::net::SocketAddr;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tracing::{debug, warn, trace, info};

use crate::crypto::{sha256, AesCtr};
use crate::crypto::random::SECURE_RANDOM;
use crate::protocol::constants::*;
use crate::protocol::tls;
use crate::stream::{FakeTlsReader, FakeTlsWriter, CryptoReader, CryptoWriter};
use crate::error::{ProxyError, HandshakeResult};
use crate::stats::ReplayChecker;
use crate::config::ProxyConfig;

/// Result of successful handshake
#[derive(Debug, Clone)]
pub struct HandshakeSuccess {
    /// Authenticated user name
    pub user: String,
    /// Target datacenter index
    pub dc_idx: i16,
    /// Protocol variant (abridged/intermediate/secure)
    pub proto_tag: ProtoTag,
    /// Decryption key and IV (for reading from client)
    pub dec_key: [u8; 32],
    pub dec_iv: u128,
    /// Encryption key and IV (for writing to client) 
    pub enc_key: [u8; 32],
    pub enc_iv: u128,
    /// Client address
    pub peer: SocketAddr,
    /// Whether TLS was used
    pub is_tls: bool,
}

/// Handle fake TLS handshake
pub async fn handle_tls_handshake<R, W>(
    handshake: &[u8],
    reader: R,
    mut writer: W,
    peer: SocketAddr,
    config: &ProxyConfig,
    replay_checker: &ReplayChecker,
) -> HandshakeResult<(FakeTlsReader<R>, FakeTlsWriter<W>, String), R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    debug!(peer = %peer, handshake_len = handshake.len(), "Processing TLS handshake");
    
    // Check minimum length
    if handshake.len() < tls::TLS_DIGEST_POS + tls::TLS_DIGEST_LEN + 1 {
        debug!(peer = %peer, "TLS handshake too short");
        return HandshakeResult::BadClient { reader, writer };
    }
    
    // Extract digest for replay check
    let digest = &handshake[tls::TLS_DIGEST_POS..tls::TLS_DIGEST_POS + tls::TLS_DIGEST_LEN];
    let digest_half = &digest[..tls::TLS_DIGEST_HALF_LEN];
    
    // Check for replay
    if replay_checker.check_tls_digest(digest_half) {
        warn!(peer = %peer, "TLS replay attack detected");
        return HandshakeResult::BadClient { reader, writer };
    }
    
    // Build secrets list
    let secrets: Vec<(String, Vec<u8>)> = config.access.users.iter()
        .filter_map(|(name, hex)| {
            hex::decode(hex).ok().map(|bytes| (name.clone(), bytes))
        })
        .collect();
    
    debug!(peer = %peer, num_users = secrets.len(), "Validating TLS handshake against users");
    
    // Validate handshake
    let validation = match tls::validate_tls_handshake(
        handshake,
        &secrets,
        config.access.ignore_time_skew,
    ) {
        Some(v) => v,
        None => {
            debug!(peer = %peer, "TLS handshake validation failed - no matching user");
            return HandshakeResult::BadClient { reader, writer };
        }
    };
    
    // Get secret for response
    let secret = match secrets.iter().find(|(name, _)| *name == validation.user) {
        Some((_, s)) => s,
        None => return HandshakeResult::BadClient { reader, writer },
    };
    
    // Build and send response
    let response = tls::build_server_hello(
        secret,
        &validation.digest,
        &validation.session_id,
        config.censorship.fake_cert_len,
    );
    
    debug!(peer = %peer, response_len = response.len(), "Sending TLS ServerHello");
    
    if let Err(e) = writer.write_all(&response).await {
        return HandshakeResult::Error(ProxyError::Io(e));
    }
    
    if let Err(e) = writer.flush().await {
        return HandshakeResult::Error(ProxyError::Io(e));
    }
    
    // Record for replay protection
    replay_checker.add_tls_digest(digest_half);
    
    info!(
        peer = %peer,
        user = %validation.user,
        "TLS handshake successful"
    );
    
    HandshakeResult::Success((
        FakeTlsReader::new(reader),
        FakeTlsWriter::new(writer),
        validation.user,
    ))
}

/// Handle MTProto obfuscation handshake
pub async fn handle_mtproto_handshake<R, W>(
    handshake: &[u8; HANDSHAKE_LEN],
    reader: R,
    writer: W,
    peer: SocketAddr,
    config: &ProxyConfig,
    replay_checker: &ReplayChecker,
    is_tls: bool,
) -> HandshakeResult<(CryptoReader<R>, CryptoWriter<W>, HandshakeSuccess), R, W>
where
    R: AsyncRead + Unpin + Send,
    W: AsyncWrite + Unpin + Send,
{
    trace!(peer = %peer, handshake = ?hex::encode(handshake), "MTProto handshake bytes");
    
    // Extract prekey and IV
    let dec_prekey_iv = &handshake[SKIP_LEN..SKIP_LEN + PREKEY_LEN + IV_LEN];
    
    debug!(
        peer = %peer,
        dec_prekey_iv = %hex::encode(dec_prekey_iv),
        "Extracted prekey+IV from handshake"
    );
    
    // Check for replay
    if replay_checker.check_handshake(dec_prekey_iv) {
        warn!(peer = %peer, "MTProto replay attack detected");
        return HandshakeResult::BadClient { reader, writer };
    }
    
    // Reversed for encryption direction
    let enc_prekey_iv: Vec<u8> = dec_prekey_iv.iter().rev().copied().collect();
    
    // Try each user's secret
    for (user, secret_hex) in &config.access.users {
        let secret = match hex::decode(secret_hex) {
            Ok(s) => s,
            Err(_) => continue,
        };
        
        // Derive decryption key
        let dec_prekey = &dec_prekey_iv[..PREKEY_LEN];
        let dec_iv_bytes = &dec_prekey_iv[PREKEY_LEN..];
        
        let mut dec_key_input = Vec::with_capacity(PREKEY_LEN + secret.len());
        dec_key_input.extend_from_slice(dec_prekey);
        dec_key_input.extend_from_slice(&secret);
        let dec_key = sha256(&dec_key_input);
        
        let dec_iv = u128::from_be_bytes(dec_iv_bytes.try_into().unwrap());
        
        // Decrypt handshake to check protocol tag
        let mut decryptor = AesCtr::new(&dec_key, dec_iv);
        let decrypted = decryptor.decrypt(handshake);
        
        trace!(
            peer = %peer,
            user = %user,
            decrypted_tail = %hex::encode(&decrypted[PROTO_TAG_POS..]),
            "Decrypted handshake tail"
        );
        
        // Check protocol tag
        let tag_bytes: [u8; 4] = decrypted[PROTO_TAG_POS..PROTO_TAG_POS + 4]
            .try_into()
            .unwrap();
        
        let proto_tag = match ProtoTag::from_bytes(tag_bytes) {
            Some(tag) => tag,
            None => {
                trace!(peer = %peer, user = %user, tag = %hex::encode(tag_bytes), "Invalid proto tag");
                continue;
            }
        };
        
        debug!(peer = %peer, user = %user, proto = ?proto_tag, "Found valid proto tag");
        
        // Check if mode is enabled
        let mode_ok = match proto_tag {
            ProtoTag::Secure => {
                if is_tls { config.general.modes.tls } else { config.general.modes.secure }
            }
            ProtoTag::Intermediate | ProtoTag::Abridged => config.general.modes.classic,
        };
        
        if !mode_ok {
            debug!(peer = %peer, user = %user, proto = ?proto_tag, "Mode not enabled");
            continue;
        }
        
        // Extract DC index
        let dc_idx = i16::from_le_bytes(
            decrypted[DC_IDX_POS..DC_IDX_POS + 2].try_into().unwrap()
        );
        
        // Derive encryption key
        let enc_prekey = &enc_prekey_iv[..PREKEY_LEN];
        let enc_iv_bytes = &enc_prekey_iv[PREKEY_LEN..];
        
        let mut enc_key_input = Vec::with_capacity(PREKEY_LEN + secret.len());
        enc_key_input.extend_from_slice(enc_prekey);
        enc_key_input.extend_from_slice(&secret);
        let enc_key = sha256(&enc_key_input);
        
        let enc_iv = u128::from_be_bytes(enc_iv_bytes.try_into().unwrap());
        
        // Record for replay protection
        replay_checker.add_handshake(dec_prekey_iv);
        
        // Create new cipher instances
        let decryptor = AesCtr::new(&dec_key, dec_iv);
        let encryptor = AesCtr::new(&enc_key, enc_iv);
        
        let success = HandshakeSuccess {
            user: user.clone(),
            dc_idx,
            proto_tag,
            dec_key,
            dec_iv,
            enc_key,
            enc_iv,
            peer,
            is_tls,
        };
        
        info!(
            peer = %peer,
            user = %user,
            dc = dc_idx,
            proto = ?proto_tag,
            tls = is_tls,
            "MTProto handshake successful"
        );
        
        return HandshakeResult::Success((
            CryptoReader::new(reader, decryptor),
            CryptoWriter::new(writer, encryptor),
            success,
        ));
    }
    
    debug!(peer = %peer, "MTProto handshake: no matching user found");
    HandshakeResult::BadClient { reader, writer }
}

/// Generate nonce for Telegram connection
/// 
/// In FAST MODE: we use the same keys for TG as for client, but reversed.
/// This means: client's enc_key becomes TG's dec_key and vice versa.
pub fn generate_tg_nonce(
    proto_tag: ProtoTag, 
    client_dec_key: &[u8; 32],
    client_dec_iv: u128,
    fast_mode: bool,
) -> ([u8; HANDSHAKE_LEN], [u8; 32], u128, [u8; 32], u128) {
    loop {
        let bytes = SECURE_RANDOM.bytes(HANDSHAKE_LEN);
        let mut nonce: [u8; HANDSHAKE_LEN] = bytes.try_into().unwrap();
        
        // Check reserved patterns
        if RESERVED_NONCE_FIRST_BYTES.contains(&nonce[0]) {
            continue;
        }
        
        let first_four: [u8; 4] = nonce[..4].try_into().unwrap();
        if RESERVED_NONCE_BEGINNINGS.contains(&first_four) {
            continue;
        }
        
        let continue_four: [u8; 4] = nonce[4..8].try_into().unwrap();
        if RESERVED_NONCE_CONTINUES.contains(&continue_four) {
            continue;
        }
        
        // Set protocol tag
        nonce[PROTO_TAG_POS..PROTO_TAG_POS + 4].copy_from_slice(&proto_tag.to_bytes());
        
        // Fast mode: copy client's dec_key+iv (this becomes TG's enc direction)
        // In fast mode, we make TG use the same keys as client but swapped:
        // - What we decrypt FROM TG = what we encrypt TO client (so no re-encryption needed)
        // - What we encrypt TO TG = what we decrypt FROM client
        if fast_mode {
            // Put client's dec_key + dec_iv into nonce[8:56]
            // This will be used by TG for encryption TO us
            nonce[SKIP_LEN..SKIP_LEN + KEY_LEN].copy_from_slice(client_dec_key);
            nonce[SKIP_LEN + KEY_LEN..SKIP_LEN + KEY_LEN + IV_LEN]
                .copy_from_slice(&client_dec_iv.to_be_bytes());
        }
        
        // Now compute what keys WE will use for TG connection
        // enc_key_iv = nonce[8:56] (for encrypting TO TG)
        // dec_key_iv = nonce[8:56] reversed (for decrypting FROM TG)
        let enc_key_iv = &nonce[SKIP_LEN..SKIP_LEN + KEY_LEN + IV_LEN];
        let dec_key_iv: Vec<u8> = enc_key_iv.iter().rev().copied().collect();
        
        let tg_enc_key: [u8; 32] = enc_key_iv[..KEY_LEN].try_into().unwrap();
        let tg_enc_iv = u128::from_be_bytes(enc_key_iv[KEY_LEN..].try_into().unwrap());
        
        let tg_dec_key: [u8; 32] = dec_key_iv[..KEY_LEN].try_into().unwrap();
        let tg_dec_iv = u128::from_be_bytes(dec_key_iv[KEY_LEN..].try_into().unwrap());
        
        debug!(
            fast_mode = fast_mode,
            tg_enc_key = %hex::encode(&tg_enc_key[..8]),
            tg_dec_key = %hex::encode(&tg_dec_key[..8]),
            "Generated TG nonce"
        );
        
        return (nonce, tg_enc_key, tg_enc_iv, tg_dec_key, tg_dec_iv);
    }
}

/// Encrypt nonce for sending to Telegram
/// 
/// Only the part from PROTO_TAG_POS onwards is encrypted.
/// The encryption key is derived from enc_key_iv in the nonce itself.
pub fn encrypt_tg_nonce(nonce: &[u8; HANDSHAKE_LEN]) -> Vec<u8> {
    // enc_key_iv is at nonce[8:56]
    let enc_key_iv = &nonce[SKIP_LEN..SKIP_LEN + KEY_LEN + IV_LEN];
    
    // Key for encrypting is just the first 32 bytes of enc_key_iv
    let key: [u8; 32] = enc_key_iv[..KEY_LEN].try_into().unwrap();
    let iv = u128::from_be_bytes(enc_key_iv[KEY_LEN..].try_into().unwrap());
    
    let mut encryptor = AesCtr::new(&key, iv);
    
    // Encrypt the entire nonce first, then take only the encrypted tail
    let encrypted_full = encryptor.encrypt(nonce);
    
    // Result: unencrypted head + encrypted tail
    let mut result = nonce[..PROTO_TAG_POS].to_vec();
    result.extend_from_slice(&encrypted_full[PROTO_TAG_POS..]);
    
    trace!(
        original = %hex::encode(&nonce[PROTO_TAG_POS..]),
        encrypted = %hex::encode(&result[PROTO_TAG_POS..]),
        "Encrypted nonce tail"
    );
    
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_generate_tg_nonce() {
        let client_dec_key = [0x42u8; 32];
        let client_dec_iv = 12345u128;
        
        let (nonce, tg_enc_key, tg_enc_iv, tg_dec_key, tg_dec_iv) = 
            generate_tg_nonce(ProtoTag::Secure, &client_dec_key, client_dec_iv, false);
        
        // Check length
        assert_eq!(nonce.len(), HANDSHAKE_LEN);
        
        // Check proto tag is set
        let tag_bytes: [u8; 4] = nonce[PROTO_TAG_POS..PROTO_TAG_POS + 4].try_into().unwrap();
        assert_eq!(ProtoTag::from_bytes(tag_bytes), Some(ProtoTag::Secure));
    }
    
    #[test]
    fn test_encrypt_tg_nonce() {
        let client_dec_key = [0x42u8; 32];
        let client_dec_iv = 12345u128;
        
        let (nonce, _, _, _, _) = 
            generate_tg_nonce(ProtoTag::Secure, &client_dec_key, client_dec_iv, false);
        
        let encrypted = encrypt_tg_nonce(&nonce);
        
        assert_eq!(encrypted.len(), HANDSHAKE_LEN);
        
        // First PROTO_TAG_POS bytes should be unchanged
        assert_eq!(&encrypted[..PROTO_TAG_POS], &nonce[..PROTO_TAG_POS]);
        
        // Rest should be different (encrypted)
        assert_ne!(&encrypted[PROTO_TAG_POS..], &nonce[PROTO_TAG_POS..]);
    }
}