//! Function-backed [`TransactionSigner`] adapter shared across chain crates.
//!
//! Every chain integration crate previously held a unit-struct
//! `XxxTransactionSigner` whose only purpose was to satisfy the
//! [`TransactionSigner`] trait by delegating one call into the
//! pure-crypto `crate::signing::sign_*` function for that chain.
//! [`FnSigner`] eliminates that ceremony: a chain plugin constructs its
//! signer inline by passing the signing function directly, and chain
//! crates can drop their `tx.rs` boilerplate entirely.
//!
//! ## Why a single non-generic type
//!
//! Some chains (EVM in particular) need to capture state — the EIP-155
//! `chain_id` is read from config at construction time and used at every
//! `sign()` call. Capturing that state requires a closure, and a closure
//! has an unnameable type that cannot appear in a struct field directly.
//! `FnSigner` therefore stores the closure behind `Box<dyn Fn ...>`
//! internally so the public type is concrete and uniform regardless of
//! whether the caller passed a function pointer or a state-carrying
//! closure. The heap allocation is paid once per plugin construction;
//! the vtable cost on `sign()` is dwarfed by the crypto work it dispatches.

use dontyeet_primitives::error::Result;
use dontyeet_primitives::secret::PrivateKey;
use dontyeet_primitives::traits::TransactionSigner;

/// Signing function type held internally.
type SignFn = dyn Fn(&[u8], &PrivateKey) -> Result<Vec<u8>> + Send + Sync;

/// [`TransactionSigner`] backed by a function pointer or closure.
pub struct FnSigner {
    sign: Box<SignFn>,
}

impl FnSigner {
    /// Wrap a signing function as a [`TransactionSigner`].
    ///
    /// `sign` is invoked verbatim on every `sign()` call. It receives
    /// the unsigned transaction bytes and the user's private key, and
    /// returns the signed payload bytes. State that does not vary per
    /// signing operation (e.g. an EIP-155 chain id) should be captured
    /// in a closure passed here at construction time.
    #[must_use]
    pub fn new<F>(sign: F) -> Self
    where
        F: Fn(&[u8], &PrivateKey) -> Result<Vec<u8>> + Send + Sync + 'static,
    {
        Self {
            sign: Box::new(sign),
        }
    }
}

impl std::fmt::Debug for FnSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FnSigner").finish_non_exhaustive()
    }
}

impl TransactionSigner for FnSigner {
    fn sign(&self, unsigned_tx: &[u8], private_key: &PrivateKey) -> Result<Vec<u8>> {
        (self.sign)(unsigned_tx, private_key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dontyeet_primitives::error::DontYeetWalletError;

    #[expect(
        clippy::unnecessary_wraps,
        reason = "test stub must match the Result<Vec<u8>> signature FnSigner expects"
    )]
    fn echo_sign(msg: &[u8], _: &PrivateKey) -> Result<Vec<u8>> {
        Ok(msg.to_vec())
    }

    fn always_fail(_: &[u8], _: &PrivateKey) -> Result<Vec<u8>> {
        Err(DontYeetWalletError::Crypto("synthetic failure".into()))
    }

    fn dummy_key() -> PrivateKey {
        PrivateKey::new(vec![0u8; 32])
    }

    #[test]
    fn round_trip_via_function_pointer() {
        let signer = FnSigner::new(echo_sign);
        let out = signer.sign(b"hello", &dummy_key()).expect("sign ok");
        assert_eq!(out, b"hello");
    }

    #[test]
    fn closure_can_capture_state() {
        let suffix = vec![0xAA, 0xBB];
        let signer = FnSigner::new(move |msg, _| {
            let mut out = msg.to_vec();
            out.extend_from_slice(&suffix);
            Ok(out)
        });
        let out = signer.sign(b"data", &dummy_key()).expect("sign ok");
        assert_eq!(out, b"data\xAA\xBB");
    }

    #[test]
    fn errors_propagate() {
        let signer = FnSigner::new(always_fail);
        let err = signer.sign(b"anything", &dummy_key()).expect_err("must fail");
        assert!(matches!(err, DontYeetWalletError::Crypto(_)));
    }

    #[test]
    fn dispatches_through_trait_object() {
        let signer = FnSigner::new(echo_sign);
        let trait_obj: &dyn TransactionSigner = &signer;
        let out = trait_obj.sign(b"trait", &dummy_key()).expect("sign ok");
        assert_eq!(out, b"trait");
    }

    #[test]
    fn debug_does_not_leak_closure_internals() {
        let signer = FnSigner::new(echo_sign);
        let rendered = format!("{signer:?}");
        assert!(rendered.starts_with("FnSigner"));
    }
}

// Rust guideline compliant 2026-05-02
