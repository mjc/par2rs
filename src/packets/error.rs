//! Error types for PAR2 packet parsing

use std::io;
use thiserror::Error;

/// Errors that can occur during PAR2 packet parsing
#[derive(Debug, Error)]
pub enum PacketParseError {
    /// Invalid magic bytes at packet start (expected "PAR2\0PKT")
    #[error("Invalid packet magic bytes: expected PAR2\\0PKT, got {0:?}")]
    InvalidMagic([u8; 8]),

    /// Packet length is invalid (must be >= 64 bytes and <= 100MB)
    ///
    /// Reference: par2cmdline-turbo/src/par2repairer.cpp:476-479
    /// Validates that packet length is at least sizeof(PACKET_HEADER)
    #[error("Invalid packet length: {0} bytes (must be 64..=104857600)")]
    InvalidLength(u64),

    /// Packet data was truncated (couldn't read full packet)
    #[error("Truncated packet: expected {expected} bytes, could only read {actual} bytes")]
    TruncatedData { expected: usize, actual: usize },

    /// I/O error while reading packet
    #[error("I/O error reading packet: {0}")]
    Io(#[from] io::Error),

    /// Unknown packet type encountered
    #[error("Unknown packet type: {0:?}")]
    UnknownPacketType([u8; 16]),

    /// Failed to parse packet data with binrw
    #[error("Failed to parse packet data: {packet_type}")]
    InvalidPacketData { packet_type: String },
}

/// Result type for packet parsing operations
pub type PacketParseResult<T> = Result<T, PacketParseError>;
