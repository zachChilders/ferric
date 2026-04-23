//! Source location tracking.

/// Represents a span of source code with start and end byte positions.
///
/// All error types must carry a Span to enable precise error reporting
/// and future renderer replacement (Rule 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    /// Starting byte position (inclusive)
    pub start: u32,
    /// Ending byte position (exclusive)
    pub end: u32,
}

impl Span {
    /// Creates a new span with the given start and end positions.
    pub fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    /// Creates a span that covers both self and other.
    pub fn to(&self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}
