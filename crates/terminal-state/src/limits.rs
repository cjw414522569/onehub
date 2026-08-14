//! Terminal escape-sequence safety limits (T081).
//!
//! Oversized OSC / DCS payloads, image and clipboard requests, hyperlinks,
//! and nested sequences must be limited so malicious terminal output cannot
//! grow memory without bound. [`EscapeLimits`] caps each kind, and
//! [`scan_corpus`] is a deterministic malicious-terminal-corpus oracle that
//! scans raw bytes for payload markers and rejects anything over the limits.
//! Memory stays bounded: scanning is linear and never buffers beyond a cap.

/// The kind of a terminal payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayloadKind {
    /// OSC payload (OSC 0..=255 ; ...).
    Osc,
    /// DCS payload (ESC P ... ST).
    Dcs,
    /// Inline image request (OSC 1337;file=..., kitty APC).
    Image,
    /// Clipboard request (OSC 52).
    Clipboard,
    /// Hyperlink URI (OSC 8).
    Hyperlink,
    /// Nested escape depth.
    Sequence,
}

/// Per-kind safety limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EscapeLimits {
    /// Maximum OSC payload length.
    pub max_osc: usize,
    /// Maximum DCS payload length.
    pub max_dcs: usize,
    /// Maximum inline image payload length.
    pub max_image: usize,
    /// Maximum clipboard request length.
    pub max_clipboard: usize,
    /// Maximum hyperlink URI length.
    pub max_hyperlink: usize,
    /// Maximum nested escape depth.
    pub max_nesting: usize,
}

impl Default for EscapeLimits {
    fn default() -> Self {
        Self {
            max_osc: 4096,
            max_dcs: 8192,
            max_image: 4 * 1024 * 1024,
            max_clipboard: 1024 * 1024,
            max_hyperlink: 2048,
            max_nesting: 8,
        }
    }
}

impl EscapeLimits {
    /// The limit for a payload kind.
    pub fn limit_for(&self, kind: PayloadKind) -> usize {
        match kind {
            PayloadKind::Osc => self.max_osc,
            PayloadKind::Dcs => self.max_dcs,
            PayloadKind::Image => self.max_image,
            PayloadKind::Clipboard => self.max_clipboard,
            PayloadKind::Hyperlink => self.max_hyperlink,
            PayloadKind::Sequence => self.max_nesting,
        }
    }
}

/// The result of checking a payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitResult {
    /// Within the limit (the length).
    Allowed(usize),
    /// Exceeded the limit.
    Exceeded {
        /// The payload kind.
        kind: PayloadKind,
        /// The configured limit.
        limit: usize,
        /// The actual length.
        actual: usize,
    },
}

/// Checks a payload length against the limit for its kind.
pub fn check_payload(kind: PayloadKind, length: usize, limits: &EscapeLimits) -> LimitResult {
    let limit = limits.limit_for(kind);
    if length > limit {
        LimitResult::Exceeded {
            kind,
            limit,
            actual: length,
        }
    } else {
        LimitResult::Allowed(length)
    }
}

/// Tracks nested escape depth (e.g. ESC inside OSC/DCS).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NestingDepth {
    current: usize,
    max: usize,
}

impl NestingDepth {
    /// A depth tracker with a maximum.
    pub fn new(max: usize) -> Self {
        Self { current: 0, max }
    }

    /// Enters a nesting level; returns false when the maximum is exceeded
    /// (the sequence must be rejected).
    pub fn enter(&mut self) -> bool {
        if self.current >= self.max {
            return false;
        }
        self.current += 1;
        true
    }

    /// Leaves a nesting level.
    pub fn leave(&mut self) {
        self.current = self.current.saturating_sub(1);
    }

    /// The current depth.
    pub fn depth(&self) -> usize {
        self.current
    }
}

/// A sequence rejected by the corpus scanner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedSequence {
    /// The payload kind.
    pub kind: PayloadKind,
    /// The length that exceeded the limit.
    pub length: usize,
    /// The configured limit.
    pub limit: usize,
    /// Byte offset of the sequence start.
    pub offset: usize,
}

/// The result of scanning a corpus.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CorpusReport {
    /// Payload sequences found.
    pub sequences: usize,
    /// Sequences within the limits.
    pub accepted: usize,
    /// Sequences rejected for exceeding a limit.
    pub rejected: Vec<RejectedSequence>,
}

/// Scans a byte stream for payload markers and validates them against the
/// limits (malicious-terminal-corpus oracle). Linear, bounded-memory scan.
pub fn scan_corpus(bytes: &[u8], limits: &EscapeLimits) -> CorpusReport {
    let mut report = CorpusReport::default();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != 0x1b {
            index += 1;
            continue;
        }
        if index + 1 >= bytes.len() {
            break;
        }
        let marker = bytes[index + 1];
        let (kind, body_start) = match marker {
            b']' => (PayloadKind::Osc, 2),
            b'P' => (PayloadKind::Dcs, 2),
            b'_' | b'^' => (PayloadKind::Osc, 2),
            _ => {
                index += 2;
                continue;
            }
        };
        let body = &bytes[index + body_start..];
        let end = payload_end(body);
        let kind = if kind == PayloadKind::Osc {
            refine_osc_kind(&body[..end])
        } else {
            kind
        };
        report.sequences += 1;
        match check_payload(kind, end, limits) {
            LimitResult::Allowed(_) => report.accepted += 1,
            LimitResult::Exceeded { limit, actual, .. } => {
                report.rejected.push(RejectedSequence {
                    kind,
                    length: actual,
                    limit,
                    offset: index,
                });
            }
        }
        // Advance past the payload and its terminator (BEL or ST).
        let terminator = if end < body.len() && body[end] == 0x07 {
            1
        } else if end < body.len() && body[end] == 0x1b && end + 1 < body.len() {
            2
        } else {
            0
        };
        index += body_start + end + terminator;
    }
    report
}

/// Refines a generic OSC payload into image / clipboard / hyperlink / osc.
fn refine_osc_kind(payload: &[u8]) -> PayloadKind {
    let text = String::from_utf8_lossy(payload);
    if text.starts_with("1337;") {
        PayloadKind::Image
    } else if text.starts_with("52;") {
        PayloadKind::Clipboard
    } else if text.starts_with("8;") {
        PayloadKind::Hyperlink
    } else {
        PayloadKind::Osc
    }
}

/// Finds the payload length until BEL or ST.
fn payload_end(body: &[u8]) -> usize {
    let mut i = 0usize;
    while i < body.len() {
        if body[i] == 0x07 {
            return i;
        }
        if body[i] == 0x1b && i + 1 < body.len() && body[i + 1] == b'\\' {
            return i;
        }
        i += 1;
    }
    body.len()
}

#[cfg(test)]
mod tests {
    use super::{check_payload, scan_corpus, EscapeLimits, LimitResult, NestingDepth, PayloadKind};

    #[test]
    fn payload_limits_are_enforced() {
        let limits = EscapeLimits::default();
        assert_eq!(
            check_payload(PayloadKind::Osc, 100, &limits),
            LimitResult::Allowed(100)
        );
        assert!(matches!(
            check_payload(PayloadKind::Osc, 5000, &limits),
            LimitResult::Exceeded {
                kind: PayloadKind::Osc,
                ..
            }
        ));
        assert!(matches!(
            check_payload(PayloadKind::Image, 10 * 1024 * 1024, &limits),
            LimitResult::Exceeded {
                kind: PayloadKind::Image,
                ..
            }
        ));
        assert!(matches!(
            check_payload(PayloadKind::Clipboard, 2 * 1024 * 1024, &limits),
            LimitResult::Exceeded {
                kind: PayloadKind::Clipboard,
                ..
            }
        ));
    }

    #[test]
    fn nesting_depth_is_bounded() {
        let mut depth = NestingDepth::new(3);
        assert!(depth.enter());
        assert!(depth.enter());
        assert!(depth.enter());
        assert!(!depth.enter(), "fourth nested level must be rejected");
        depth.leave();
        assert!(depth.enter(), "leaving restores capacity");
    }

    #[test]
    fn malicious_corpus_rejects_oversized_sequences() {
        let limits = EscapeLimits {
            max_osc: 8,
            max_dcs: 8,
            max_image: 16,
            max_clipboard: 8,
            max_hyperlink: 8,
            max_nesting: 8,
        };
        // Oversized OSC, DCS, image, clipboard, and hyperlink payloads.
        let mut corpus = Vec::new();
        corpus.extend_from_slice(b"\x1b]0;");
        corpus.extend_from_slice(&[b'x'; 100]);
        corpus.push(0x07);
        corpus.extend_from_slice(b"\x1bP");
        corpus.extend_from_slice(&[b'y'; 50]);
        corpus.extend_from_slice(b"\x1b\\");
        corpus.extend_from_slice(b"\x1b]1337;file=base64,");
        corpus.extend_from_slice(&[b'z'; 200]);
        corpus.push(0x07);
        corpus.extend_from_slice(b"\x1b]52;c;");
        corpus.extend_from_slice(&[b'w'; 40]);
        corpus.push(0x07);
        corpus.extend_from_slice(b"\x1b]8;;https://a/");
        corpus.extend_from_slice(&[b'u'; 30]);
        corpus.push(0x07);

        let report = scan_corpus(&corpus, &limits);
        assert_eq!(report.sequences, 5);
        assert_eq!(report.accepted, 0);
        assert_eq!(
            report.rejected.len(),
            5,
            "every oversized sequence rejected"
        );
        let kinds: Vec<PayloadKind> = report.rejected.iter().map(|r| r.kind).collect();
        assert!(kinds.contains(&PayloadKind::Osc));
        assert!(kinds.contains(&PayloadKind::Dcs));
        assert!(kinds.contains(&PayloadKind::Image));
        assert!(kinds.contains(&PayloadKind::Clipboard));
        assert!(kinds.contains(&PayloadKind::Hyperlink));
    }

    #[test]
    fn benign_corpus_is_accepted_and_memory_bounded() {
        let limits = EscapeLimits::default();
        let corpus = b"\x1b]0;title\x07\x1bPdata\x1b\\plain\x1b]8;;https://example.com\x07";
        let report = scan_corpus(corpus, &limits);
        assert_eq!(report.accepted, 3);
        assert!(report.rejected.is_empty());
    }

    #[test]
    fn million_byte_corpus_scan_is_bounded() {
        // A huge malicious stream: scanning must not allocate unboundedly.
        let limits = EscapeLimits::default();
        let mut corpus = vec![0x1b, b']', b'0', b';'];
        corpus.extend(std::iter::repeat_n(b'x', 1_000_000));
        corpus.push(0x07);
        let report = scan_corpus(&corpus, &limits);
        assert_eq!(report.rejected.len(), 1);
        assert_eq!(report.rejected[0].kind, PayloadKind::Osc);
        assert!(report.rejected[0].length >= limits.max_osc);
    }
}
