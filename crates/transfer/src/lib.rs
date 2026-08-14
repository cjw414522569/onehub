#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # transfer
//!
//! Bounded-memory streaming transfer engine (T057): chunked pipeline with
//! concurrent in-flight chunks, backpressure, and cooperative yielding so
//! interactive sessions are never starved.

pub mod streaming;

pub use streaming::{
    run_streaming_copy, ChunkReader, ChunkWriter, StreamConfig, TransferStats, DEFAULT_CHUNK_SIZE,
    DEFAULT_MAX_IN_FLIGHT,
};

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "transfer";
