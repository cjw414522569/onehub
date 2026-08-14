#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # secret
//!
//! Sensitive value types with automatic zeroization on drop.
//!
//! `SecretBytes` and `SecretString` deliberately do **not** implement
//! `Debug`, `Display`, `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash`, or
//! `Serialize`, so secret material cannot be accidentally formatted,
//! duplicated, compared, or serialized. Access is explicit via
//! `expose_secret()`.
//!
//! ```compile_fail
//! // SecretString must not implement Debug.
//! let value = secret::SecretString::from_string(String::from("hunter2"));
//! let _ = format!("{:?}", value);
//! ```
//!
//! ```compile_fail
//! // SecretBytes must not implement Display.
//! let value = secret::SecretBytes::from_vec(vec![1, 2, 3]);
//! let _ = format!("{}", value);
//! ```
//!
//! ```compile_fail
//! // SecretString must not be cloneable.
//! let value = secret::SecretString::from_string(String::from("hunter2"));
//! let _clone = value.clone();
//! ```
//!
//! ```compile_fail
//! // SecretBytes must not be copied.
//! let value = secret::SecretBytes::from_vec(vec![1, 2, 3]);
//! let _copy = value;
//! let _used_again = value;
//! ```

use zeroize::Zeroize;

/// A sensitive byte buffer.
///
/// Stored as `Box<[u8]>` so there is no spare capacity that could retain a
/// copy after zeroization. The buffer is zeroized on drop.
pub struct SecretBytes {
    inner: Box<[u8]>,
}

/// Zeroizes a boxed buffer in place using the same code path `Drop` runs.
fn clear_buffer(buffer: &mut Box<[u8]>) {
    buffer.zeroize();
    // Prevent the optimizer from treating the zeroing as a dead store.
    std::hint::black_box(buffer);
}

impl SecretBytes {
    /// Creates a secret buffer from an owned vector.
    pub fn from_vec(bytes: Vec<u8>) -> Self {
        Self {
            inner: bytes.into_boxed_slice(),
        }
    }

    /// Creates a secret buffer from a slice.
    pub fn from_slice(bytes: &[u8]) -> Self {
        Self::from_vec(bytes.to_vec())
    }

    /// Returns the number of bytes.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Explicitly exposes the secret bytes.
    ///
    /// Callers must treat the returned slice as secret material.
    pub fn expose_secret(&self) -> &[u8] {
        &self.inner
    }
}

impl Drop for SecretBytes {
    fn drop(&mut self) {
        clear_buffer(&mut self.inner);
    }
}

/// A sensitive UTF-8 string.
///
/// Backed by `Box<[u8]>` (no spare capacity); the bytes are zeroized on drop.
/// UTF-8 validity is guaranteed by construction.
pub struct SecretString {
    inner: Box<[u8]>,
}

impl SecretString {
    /// Creates a secret string from an owned `String`.
    pub fn from_string(value: String) -> Self {
        Self {
            inner: value.into_bytes().into_boxed_slice(),
        }
    }

    /// Creates a secret string from UTF-8 bytes, validating encoding.
    pub fn try_from_utf8(bytes: Vec<u8>) -> Result<Self, std::str::Utf8Error> {
        let _validated = std::str::from_utf8(&bytes)?;
        Ok(Self {
            inner: bytes.into_boxed_slice(),
        })
    }

    /// Returns the number of bytes.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns whether the string is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Explicitly exposes the secret string.
    ///
    /// UTF-8 is guaranteed by construction, so this never fails.
    pub fn expose_secret(&self) -> &str {
        std::str::from_utf8(&self.inner).expect("SecretString always contains valid UTF-8")
    }

    /// Explicitly exposes the secret bytes.
    pub fn expose_bytes(&self) -> &[u8] {
        &self.inner
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        clear_buffer(&mut self.inner);
    }
}

impl From<String> for SecretString {
    fn from(value: String) -> Self {
        Self::from_string(value)
    }
}

impl From<&str> for SecretString {
    fn from(value: &str) -> Self {
        Self::from_string(value.to_owned())
    }
}

impl From<Vec<u8>> for SecretBytes {
    fn from(value: Vec<u8>) -> Self {
        Self::from_vec(value)
    }
}

impl From<&[u8]> for SecretBytes {
    fn from(value: &[u8]) -> Self {
        Self::from_slice(value)
    }
}

#[cfg(test)]
mod tests {
    use super::{clear_buffer, SecretBytes, SecretString};

    #[test]
    fn secret_bytes_expose_and_length() {
        let secret = SecretBytes::from_vec(vec![1, 2, 3, 4]);
        assert_eq!(secret.len(), 4);
        assert!(!secret.is_empty());
        assert_eq!(secret.expose_secret(), &[1, 2, 3, 4]);
    }

    #[test]
    fn secret_string_expose_and_validation() {
        let secret = SecretString::from_string(String::from("correct horse battery staple"));
        assert_eq!(secret.len(), 28);
        assert_eq!(secret.expose_secret(), "correct horse battery staple");
        assert_eq!(secret.expose_bytes(), b"correct horse battery staple");

        let invalid = SecretString::try_from_utf8(vec![0xff, 0xfe]);
        assert!(invalid.is_err());
        let valid = SecretString::try_from_utf8(String::from("ok").into_bytes());
        assert_eq!(valid.expect("valid utf8").expose_secret(), "ok");
    }

    #[test]
    fn zeroization_clears_the_entire_buffer() {
        // Verifies the exact code path `Drop` runs (clear_buffer + zeroize +
        // black_box) on a buffer we still own and can observe safely.
        let mut buffer = vec![0x55u8; 32].into_boxed_slice();
        assert!(buffer.iter().all(|byte| *byte == 0x55));
        clear_buffer(&mut buffer);
        assert!(
            buffer.iter().all(|byte| *byte == 0),
            "clear_buffer must zero every byte"
        );
        assert_eq!(buffer.len(), 32);
    }

    #[test]
    fn empty_secret_values_are_supported() {
        let bytes = SecretBytes::from_vec(Vec::new());
        assert!(bytes.is_empty());
        let string = SecretString::from_string(String::new());
        assert!(string.is_empty());
        assert_eq!(string.expose_secret(), "");
    }

    #[test]
    fn secret_types_can_be_moved_and_dropped_without_leak_tools() {
        // Move semantics still work (move does not copy the buffer), and
        // dropping runs zeroization through the normal path.
        let first = SecretString::from_string(String::from("moved-value"));
        let second = first;
        assert_eq!(second.expose_secret(), "moved-value");
    }
}
