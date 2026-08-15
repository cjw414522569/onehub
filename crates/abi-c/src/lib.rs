#![doc = include_str!("../README.md")]
// NOTE: this crate intentionally does NOT `forbid(unsafe_code)`: the stable C
// ABI must export `#[no_mangle] extern "C"` symbols, and `no_mangle` is an
// unsafe attribute. There are no unsafe blocks anywhere in this crate; the
// only unsafe items are the export attributes themselves.

//! # abi-c
//!
//! Stable, versioned C ABI (T097).
//!
//! The ABI is a fixed [`AbiMessageHeader`] layout plus a small set of
//! `extern "C"` functions. The canonical C header
//! `crates/abi-c/include/ssh_abi.h` is **generated** by
//! `scripts/generate-abi.mjs` and checked in; the codegen version is pinned
//! ([`ABI_CODEGEN_VERSION`]) and the generator is deterministic, so CI can
//! detect drift by regenerating and diffing. Rust tests parse the generated
//! header and assert the layout matches the exported ABI — the host-side
//! equivalent of a C link test (no C toolchain is required).
//!
//! The ABI honors the architecture contract: every message carries
//! `schema_version`, `message_type`, `byte_len`, `request_id`, `cancel`,
//! `backpressure`, and `error_code`; ownership is creator-owned with
//! idempotent release; no raw pointers, borrowed references, exceptions,
//! closures, or secret debug values cross the boundary (the exported
//! functions take and return plain values; the only pointer returned is the
//! pinned codegen-version string).

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "abi-c";

pub mod event_stream;
pub mod handle;
pub mod scheduler;

pub use event_stream::{
    BatchItem, EventBatch, EventStream, PushResult, EVENT_BATCH_MAX_BYTES, EVENT_BATCH_MAX_EVENTS,
    EVENT_BATCH_VERSION,
};
pub use handle::{HandleResource, HandleTable, INVALID_HANDLE};
pub use scheduler::{Scheduler, UiScheduler, WindowsUiScheduler};

/// ABI schema version (1).
pub const ABI_SCHEMA_VERSION: u32 = 1;

/// Pinned codegen version that produced the C header and ABI surface.
pub const ABI_CODEGEN_VERSION: &str = "1.0.0";

/// Field ids used by [`ssh_abi_field_offset`].
pub const FIELD_SCHEMA_VERSION: u32 = 0;
/// Field id for `message_type`.
pub const FIELD_MESSAGE_TYPE: u32 = 1;
/// Field id for `byte_len`.
pub const FIELD_BYTE_LEN: u32 = 2;
/// Field id for `request_id`.
pub const FIELD_REQUEST_ID: u32 = 3;
/// Field id for `cancel`.
pub const FIELD_CANCEL: u32 = 4;
/// Field id for `backpressure`.
pub const FIELD_BACKPRESSURE: u32 = 5;
/// Field id for `error_code`.
pub const FIELD_ERROR_CODE: u32 = 6;
/// Number of header fields.
pub const FIELD_COUNT: u32 = 7;

/// The versioned message header (layout fixed by the generated C header).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AbiMessageHeader {
    /// ABI schema version (must equal [`ABI_SCHEMA_VERSION`]).
    pub schema_version: u32,
    /// Message type discriminator.
    pub message_type: u32,
    /// Payload byte length following the header.
    pub byte_len: u64,
    /// Caller-chosen request id (0 = none).
    pub request_id: u64,
    /// Cancellation flag (0 = run, 1 = cancel).
    pub cancel: u8,
    /// Backpressure flag (0 = ready, 1 = slow consumer).
    pub backpressure: u8,
    /// Error code (0 = ok).
    pub error_code: u32,
}

impl Default for AbiMessageHeader {
    fn default() -> Self {
        Self {
            schema_version: ABI_SCHEMA_VERSION,
            message_type: 0,
            byte_len: 0,
            request_id: 0,
            cancel: 0,
            backpressure: 0,
            error_code: 0,
        }
    }
}

impl AbiMessageHeader {
    /// A header for a message with a payload.
    pub fn new(message_type: u32, byte_len: u64, request_id: u64) -> Self {
        Self {
            schema_version: ABI_SCHEMA_VERSION,
            message_type,
            byte_len,
            request_id,
            cancel: 0,
            backpressure: 0,
            error_code: 0,
        }
    }

    /// Whether the header carries the current schema version.
    pub fn is_valid(&self) -> bool {
        self.schema_version == ABI_SCHEMA_VERSION
    }
}

static CODEGEN_VERSION_BYTES: [u8; 6] = *b"1.0.0\0";

/// The pinned codegen version bytes (safe accessor; the C export points to
/// this static).
pub fn codegen_version_bytes() -> &'static [u8] {
    &CODEGEN_VERSION_BYTES
}

fn field_suffix(field: u32) -> &'static str {
    match field {
        FIELD_SCHEMA_VERSION => "SCHEMA_VERSION",
        FIELD_MESSAGE_TYPE => "MESSAGE_TYPE",
        FIELD_BYTE_LEN => "BYTE_LEN",
        FIELD_REQUEST_ID => "REQUEST_ID",
        FIELD_CANCEL => "CANCEL",
        FIELD_BACKPRESSURE => "BACKPRESSURE",
        FIELD_ERROR_CODE => "ERROR_CODE",
        _ => "",
    }
}

fn offset_macro_name(field: u32) -> String {
    format!("SSH_ABI_OFFSET_{}", field_suffix(field))
}

/// Parses a `#define NAME value` integer macro from the generated header.
fn header_macro_u64(header: &str, name: &str) -> Option<u64> {
    header.lines().find_map(|line| {
        let line = line.trim();
        let prefix = format!("#define {name} ");
        line.strip_prefix(&prefix).and_then(|value| {
            let value = value.trim().trim_end_matches('u');
            value.parse().ok()
        })
    })
}

/// The ABI schema version exported to C.
#[no_mangle]
pub extern "C" fn ssh_abi_version() -> u32 {
    ABI_SCHEMA_VERSION
}

/// The pinned codegen version string (NUL-terminated), exported to C.
#[no_mangle]
pub extern "C" fn ssh_abi_codegen_version() -> *const u8 {
    std::ptr::addr_of!(CODEGEN_VERSION_BYTES).cast::<u8>()
}

/// The size of [`AbiMessageHeader`] in bytes.
#[no_mangle]
pub extern "C" fn ssh_abi_header_size() -> u64 {
    std::mem::size_of::<AbiMessageHeader>() as u64
}

/// The byte offset of a header field, or `u64::MAX` for unknown fields.
#[no_mangle]
pub extern "C" fn ssh_abi_field_offset(field: u32) -> u64 {
    match field {
        FIELD_SCHEMA_VERSION => std::mem::offset_of!(AbiMessageHeader, schema_version) as u64,
        FIELD_MESSAGE_TYPE => std::mem::offset_of!(AbiMessageHeader, message_type) as u64,
        FIELD_BYTE_LEN => std::mem::offset_of!(AbiMessageHeader, byte_len) as u64,
        FIELD_REQUEST_ID => std::mem::offset_of!(AbiMessageHeader, request_id) as u64,
        FIELD_CANCEL => std::mem::offset_of!(AbiMessageHeader, cancel) as u64,
        FIELD_BACKPRESSURE => std::mem::offset_of!(AbiMessageHeader, backpressure) as u64,
        FIELD_ERROR_CODE => std::mem::offset_of!(AbiMessageHeader, error_code) as u64,
        _ => u64::MAX,
    }
}

/// Validates a schema version: `0` when it matches, `-1` otherwise.
#[no_mangle]
pub extern "C" fn ssh_abi_header_is_valid(schema_version: u32) -> i32 {
    if schema_version == ABI_SCHEMA_VERSION {
        0
    } else {
        -1
    }
}

/// Verifies that the generated header matches the exported ABI layout.
/// Returns the number of mismatches (`0` = header and ABI are consistent).
/// A C consumer calls this once at startup as a link/ABI self-check.
#[no_mangle]
pub extern "C" fn ssh_abi_validate_field_offsets() -> i32 {
    let header = include_str!("../include/ssh_abi.h");
    let mut mismatches = 0i32;
    if header_macro_u64(header, "SSH_ABI_SCHEMA_VERSION") != Some(ABI_SCHEMA_VERSION as u64) {
        mismatches += 1;
    }
    if header_macro_u64(header, "SSH_ABI_HEADER_SIZE")
        != Some(std::mem::size_of::<AbiMessageHeader>() as u64)
    {
        mismatches += 1;
    }
    let offsets = [
        (
            FIELD_SCHEMA_VERSION,
            std::mem::offset_of!(AbiMessageHeader, schema_version),
        ),
        (
            FIELD_MESSAGE_TYPE,
            std::mem::offset_of!(AbiMessageHeader, message_type),
        ),
        (
            FIELD_BYTE_LEN,
            std::mem::offset_of!(AbiMessageHeader, byte_len),
        ),
        (
            FIELD_REQUEST_ID,
            std::mem::offset_of!(AbiMessageHeader, request_id),
        ),
        (FIELD_CANCEL, std::mem::offset_of!(AbiMessageHeader, cancel)),
        (
            FIELD_BACKPRESSURE,
            std::mem::offset_of!(AbiMessageHeader, backpressure),
        ),
        (
            FIELD_ERROR_CODE,
            std::mem::offset_of!(AbiMessageHeader, error_code),
        ),
    ];
    for (field, offset) in offsets {
        let name = offset_macro_name(field);
        if header_macro_u64(header, &name) != Some(offset as u64) {
            mismatches += 1;
        }
    }
    mismatches
}

#[cfg(test)]
mod tests {
    use super::{
        header_macro_u64, AbiMessageHeader, ABI_SCHEMA_VERSION, FIELD_BACKPRESSURE, FIELD_BYTE_LEN,
        FIELD_CANCEL, FIELD_COUNT, FIELD_ERROR_CODE, FIELD_MESSAGE_TYPE, FIELD_REQUEST_ID,
        FIELD_SCHEMA_VERSION,
    };

    const HEADER: &str = include_str!("../include/ssh_abi.h");

    #[test]
    fn header_layout_matches_rust_layout() {
        // The host-side equivalent of a C link test: the generated header's
        // documented size and offsets must equal the exported Rust layout.
        assert_eq!(
            header_macro_u64(HEADER, "SSH_ABI_HEADER_SIZE"),
            Some(std::mem::size_of::<AbiMessageHeader>() as u64)
        );
        assert_eq!(std::mem::size_of::<AbiMessageHeader>(), 32);
        assert_eq!(std::mem::align_of::<AbiMessageHeader>(), 8);
        let expected_offsets = [
            (FIELD_SCHEMA_VERSION, 0u64),
            (FIELD_MESSAGE_TYPE, 4),
            (FIELD_BYTE_LEN, 8),
            (FIELD_REQUEST_ID, 16),
            (FIELD_CANCEL, 24),
            (FIELD_BACKPRESSURE, 25),
            (FIELD_ERROR_CODE, 28),
        ];
        for (field, expected) in expected_offsets {
            let name = format!("SSH_ABI_OFFSET_{}", super::field_suffix(field));
            assert_eq!(
                header_macro_u64(HEADER, &name),
                Some(expected),
                "offset macro {name} must match the Rust layout"
            );
            assert_eq!(
                super::ssh_abi_field_offset(field),
                expected,
                "exported field offset must match the header"
            );
        }
        assert_eq!(
            header_macro_u64(HEADER, "SSH_ABI_FIELD_COUNT"),
            Some(FIELD_COUNT as u64)
        );
        assert_eq!(super::ssh_abi_field_offset(99), u64::MAX);
    }

    #[test]
    fn exported_functions_agree_with_header() {
        assert_eq!(super::ssh_abi_version(), ABI_SCHEMA_VERSION);
        assert_eq!(
            header_macro_u64(HEADER, "SSH_ABI_SCHEMA_VERSION"),
            Some(ABI_SCHEMA_VERSION as u64)
        );
        assert_eq!(
            super::ssh_abi_header_size(),
            std::mem::size_of::<AbiMessageHeader>() as u64
        );
        assert_eq!(super::ssh_abi_header_is_valid(ABI_SCHEMA_VERSION), 0);
        assert_eq!(super::ssh_abi_header_is_valid(2), -1);
    }

    #[test]
    fn codegen_version_is_pinned_and_embedded() {
        // The exported C pointer points at the pinned NUL-terminated bytes.
        assert_eq!(super::codegen_version_bytes(), b"1.0.0\0");
        assert_eq!(
            super::ssh_abi_codegen_version(),
            super::codegen_version_bytes().as_ptr(),
            "the C export must point at the pinned version string"
        );
        assert!(HEADER.contains("#define SSH_ABI_CODEGEN_VERSION \"1.0.0\""));
        assert!(HEADER.contains("Generated by scripts/generate-abi.mjs"));
    }

    #[test]
    fn message_header_default_and_validity() {
        let header = AbiMessageHeader::new(7, 128, 42);
        assert_eq!(header.schema_version, ABI_SCHEMA_VERSION);
        assert_eq!(header.message_type, 7);
        assert_eq!(header.byte_len, 128);
        assert_eq!(header.request_id, 42);
        assert!(header.is_valid());
        assert_eq!(AbiMessageHeader::default().error_code, 0);
        let mut stale = header;
        stale.schema_version = 0;
        assert!(!stale.is_valid());
    }

    #[test]
    fn validate_field_offsets_returns_zero() {
        // A C consumer calls this at startup; it must be consistent.
        assert_eq!(super::ssh_abi_validate_field_offsets(), 0);
    }
}
