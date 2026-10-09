//! Compatibility with the ECDH shared key that dashj (Android's legacy
//! library) derives for DashPay.
//!
//! dashj computes the same DIP-15 hash as [`derive_shared_key_ecdh`]
//! (`SHA256((y&1|2) ‖ x)`), but passes the 32 bytes through a signed Java
//! `BigInteger` before using them as the AES key. Since dashj #295 it pads the
//! result back to 32 bytes with `BigIntegers.asUnsignedByteArray(32, …)`. For
//! almost every key that is a no-op. It is not when the hash starts with `0xFF`
//! followed by a byte with the high bit set: read as a signed number, those
//! leading `0xFF` bytes are redundant sign bytes, `toByteArray()` drops them,
//! and the padding puts `0x00` back in their place. So `ff 80 …` becomes
//! `00 80 …` and `ff ff 80 …` becomes `00 00 80 …`. About 1 contact pair in 512
//! is affected.
//!
//! dashj encrypts with that altered key, so a request it sent to such a pair
//! does not decrypt with the standard key. The fix on the receiving side is to
//! try the standard key first and, only if that fails, the dashj variant. We
//! always encrypt with the standard key.
//!
//! [`derive_shared_key_ecdh`]: crate::derive_shared_key_ecdh

use zeroize::Zeroizing;

/// Which shared key a fallback decrypt succeeded with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharedKeyVariant {
    /// The DIP-15 key from [`crate::derive_shared_key_ecdh`].
    Standard,
    /// The dashj variant from [`dashj_legacy_shared_key`].
    DashjLegacy,
}

/// The key dashj derives for the same ECDH exchange, when it differs from
/// `standard`.
///
/// Rule: starting at byte 0, while `key[i] == 0xFF` and `key[i + 1]` has its
/// high bit set, `key[i]` becomes `0x00`; stop at the first byte where that
/// does not hold. This is exactly what a signed `BigInteger` round trip plus
/// left zero-padding to 32 bytes does.
///
/// Returns `None` when the dashj key equals the standard key (about 511 keys in
/// 512), so callers can skip a second decrypt attempt.
pub fn dashj_legacy_shared_key(standard: &[u8; 32]) -> Option<[u8; 32]> {
    if standard[0] != 0xFF || standard[1] & 0x80 == 0 {
        return None;
    }
    let mut legacy = *standard;
    let mut i = 0;
    while i + 1 < legacy.len() && standard[i] == 0xFF && standard[i + 1] & 0x80 != 0 {
        legacy[i] = 0x00;
        i += 1;
    }
    Some(legacy)
}

/// Run `attempt` with the standard shared key, and if it fails, once more with
/// the dashj variant of that key (when there is one).
///
/// `attempt` should fail whenever the plaintext is not what it expects (bad
/// padding, wrong length, unparseable), not only on a padding error: AES-CBC
/// has no integrity check, so a wrong key gets past the padding check about
/// 1 time in 256.
///
/// On success, returns the value and which key produced it. When both keys
/// fail, returns the error from the standard key, so the caller's message
/// stays the same as before this fallback existed.
pub fn decrypt_with_dashj_fallback<T, E>(
    standard: &[u8; 32],
    mut attempt: impl FnMut(&[u8; 32]) -> Result<T, E>,
) -> Result<(T, SharedKeyVariant), E> {
    let standard_err = match attempt(standard) {
        Ok(value) => return Ok((value, SharedKeyVariant::Standard)),
        Err(e) => e,
    };
    let Some(legacy) = dashj_legacy_shared_key(standard).map(Zeroizing::new) else {
        return Err(standard_err);
    };
    match attempt(&legacy) {
        Ok(value) => Ok((value, SharedKeyVariant::DashjLegacy)),
        Err(_) => Err(standard_err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        compact_xpub_bytes, decrypt_extended_public_key, derive_shared_key_ecdh,
        encrypt_extended_public_key, parse_compact_xpub, CompactXpub, CryptoError,
    };
    use secp256k1::{PublicKey, Secp256k1, SecretKey};

    fn key_with_prefix(prefix: &[u8]) -> [u8; 32] {
        let mut key = [0x5Au8; 32];
        key[..prefix.len()].copy_from_slice(prefix);
        key
    }

    #[test]
    fn ff_then_high_bit_drops_the_leading_ff() {
        let standard = key_with_prefix(&[0xFF, 0x80]);
        let legacy = dashj_legacy_shared_key(&standard).expect("differs");
        assert_eq!(legacy, key_with_prefix(&[0x00, 0x80]));
    }

    #[test]
    fn two_leading_ff_both_dropped() {
        let standard = key_with_prefix(&[0xFF, 0xFF, 0x80]);
        let legacy = dashj_legacy_shared_key(&standard).expect("differs");
        assert_eq!(legacy, key_with_prefix(&[0x00, 0x00, 0x80]));
    }

    #[test]
    fn ff_ff_7f_drops_only_the_first_ff() {
        // The second 0xFF is followed by 0x7F, so it is the sign byte and stays.
        let standard = key_with_prefix(&[0xFF, 0xFF, 0x7F]);
        let legacy = dashj_legacy_shared_key(&standard).expect("differs");
        assert_eq!(legacy, key_with_prefix(&[0x00, 0xFF, 0x7F]));
    }

    #[test]
    fn ff_then_low_byte_is_unchanged() {
        assert_eq!(
            dashj_legacy_shared_key(&key_with_prefix(&[0xFF, 0x7F])),
            None
        );
    }

    #[test]
    fn leading_zero_is_unchanged() {
        assert_eq!(
            dashj_legacy_shared_key(&key_with_prefix(&[0x00, 0x80])),
            None
        );
    }

    #[test]
    fn ordinary_key_is_unchanged() {
        assert_eq!(dashj_legacy_shared_key(&[0x5Au8; 32]), None);
        assert_eq!(
            dashj_legacy_shared_key(&key_with_prefix(&[0x80, 0xFF])),
            None
        );
    }

    #[test]
    fn all_ff_keeps_only_the_last_byte() {
        // -1 as a signed BigInteger is the single byte 0xFF.
        let mut expected = [0u8; 32];
        expected[31] = 0xFF;
        assert_eq!(dashj_legacy_shared_key(&[0xFF; 32]), Some(expected));
    }

    #[test]
    fn fallback_does_not_retry_when_keys_match() {
        let mut calls = 0;
        let result: Result<((), SharedKeyVariant), &str> =
            decrypt_with_dashj_fallback(&[0x5Au8; 32], |_| {
                calls += 1;
                Err("no")
            });
        assert_eq!(result.unwrap_err(), "no");
        assert_eq!(calls, 1, "no second attempt when the dashj key is the same");
    }

    #[test]
    fn fallback_reports_the_standard_error_when_both_fail() {
        let standard = key_with_prefix(&[0xFF, 0x80]);
        let result: Result<((), SharedKeyVariant), u8> =
            decrypt_with_dashj_fallback(&standard, |k| Err(k[0]));
        assert_eq!(result.unwrap_err(), 0xFF, "error from the standard key");
    }

    /// Fixed test vector. Private keys `A = [0xA1; 32]` and `B = 599` (as a
    /// 32-byte big-endian scalar) give the ECDH key below, which starts `ff a4`
    /// — a pair dashj gets wrong. 599 is the first `B` from 1 upward with that
    /// property for this `A`.
    const AFFECTED_A: [u8; 32] = [0xA1; 32];
    const AFFECTED_B_SCALAR: u64 = 599;
    const AFFECTED_STANDARD_KEY: &str =
        "ffa4f406d481e975d08a9100935feca3a9ec39278f925a990e4e5b0e3dc4e5b3";

    fn scalar_key(n: u64) -> SecretKey {
        let mut bytes = [0u8; 32];
        bytes[24..].copy_from_slice(&n.to_be_bytes());
        SecretKey::from_slice(&bytes).expect("valid scalar")
    }

    fn hex_string(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn dashj_encrypted_xpub_decrypts_with_fallback() {
        let secp = Secp256k1::new();
        let a = SecretKey::from_slice(&AFFECTED_A).unwrap();
        let b = scalar_key(AFFECTED_B_SCALAR);
        let pub_a = PublicKey::from_secret_key(&secp, &a);
        let pub_b = PublicKey::from_secret_key(&secp, &b);

        // Both sides derive the same standard key, and it is an affected one.
        let standard = derive_shared_key_ecdh(&b, &pub_a);
        assert_eq!(standard, derive_shared_key_ecdh(&a, &pub_b));
        assert_eq!(hex_string(&standard), AFFECTED_STANDARD_KEY);
        let legacy = dashj_legacy_shared_key(&standard).expect("affected pair");
        assert_eq!(
            hex_string(&legacy),
            "00a4f406d481e975d08a9100935feca3a9ec39278f925a990e4e5b0e3dc4e5b3"
        );

        // dashj (sender B) encrypts the compact xpub with its altered key.
        let mut pubkey = [0x33u8; 33];
        pubkey[0] = 0x02;
        let xpub = compact_xpub_bytes([1, 2, 3, 4], [0x77; 32], pubkey);
        let ciphertext = encrypt_extended_public_key(&legacy, &[0x0Fu8; 16], &xpub);

        let open = |key: &[u8; 32]| -> Result<CompactXpub, CryptoError> {
            parse_compact_xpub(&decrypt_extended_public_key(key, &ciphertext)?)
        };

        // The standard key alone cannot open it.
        assert!(open(&standard).is_err());

        // Receiver A, with the fallback, recovers the exact xpub.
        let (parsed, variant) =
            decrypt_with_dashj_fallback(&derive_shared_key_ecdh(&a, &pub_b), open)
                .expect("fallback recovers dashj ciphertext");
        assert_eq!(variant, SharedKeyVariant::DashjLegacy);
        assert_eq!(parsed.to_bytes(), xpub);

        // A standard-key ciphertext for the same pair still opens with the
        // standard key first.
        let standard_ct = encrypt_extended_public_key(&standard, &[0x0Fu8; 16], &xpub);
        let (_, variant) = decrypt_with_dashj_fallback(&standard, |k| {
            parse_compact_xpub(&decrypt_extended_public_key(k, &standard_ct)?)
        })
        .expect("standard decrypt");
        assert_eq!(variant, SharedKeyVariant::Standard);
    }
}
