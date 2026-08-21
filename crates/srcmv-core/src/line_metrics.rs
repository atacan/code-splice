use std::sync::Arc;

use crate::{ByteRange, CoreError};

/// Compact line-boundary data derived from one immutable byte slice.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LineIndex {
    /// Exclusive byte end of each logical line, including its terminator.
    boundaries: Arc<[u64]>,
}

impl LineIndex {
    /// Builds a line index for immutable bytes while enforcing representation limits.
    ///
    /// LF, CRLF, and lone CR are terminators. A nonempty unterminated suffix is a
    /// line, while an empty file and the suffix after a final terminator are not.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::ResourceLimitExceeded`] when adding a boundary would
    /// exceed `maximum_lines` or `maximum_memory_bytes`.
    pub fn from_bytes_with_limits(
        bytes: &[u8],
        maximum_lines: u64,
        maximum_memory_bytes: u64,
    ) -> Result<Self, CoreError> {
        let byte_length =
            u64::try_from(bytes.len()).map_err(|_| line_limit_overflow("snapshot_file_bytes"))?;
        let mut boundaries = Vec::new();
        let mut offset = 0;

        while let Some(boundary) = next_terminator_end(bytes, &mut offset) {
            push_line_boundary(
                &mut boundaries,
                boundary,
                maximum_lines,
                maximum_memory_bytes,
            )?;
        }

        if boundaries.last().copied() != Some(byte_length) && !bytes.is_empty() {
            push_line_boundary(
                &mut boundaries,
                bytes.len(),
                maximum_lines,
                maximum_memory_bytes,
            )?;
        }

        Ok(Self {
            boundaries: boundaries.into(),
        })
    }

    /// Returns metrics for `range` with the selected bytes interpreted independently.
    ///
    /// The supplied `bytes` must be the same complete byte slice from which this
    /// index was built. The range is zero-based and half-open. Empty ranges are
    /// valid and have zero bytes and zero lines. A range ending between the CR and
    /// LF of an indexed CRLF sequence treats its final CR as a lone terminator; a
    /// range starting at that LF treats the LF as a terminator.
    ///
    /// This query uses binary searches over the index and inspects only the edge
    /// bytes of the range; it does not rescan the selected range.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidDomainValue`] when the byte length differs from
    /// the indexed length or when the range is invalid. Returns
    /// [`CoreError::ResourceLimitExceeded`] if a byte or line count cannot be
    /// represented.
    pub fn metrics_for_range(
        &self,
        bytes: &[u8],
        range: ByteRange,
    ) -> Result<LineMetrics, CoreError> {
        let byte_length =
            u64::try_from(bytes.len()).map_err(|_| line_limit_overflow("line_metric_bytes"))?;
        if self.indexed_byte_length() != byte_length {
            return Err(CoreError::InvalidDomainValue {
                field: "line_metrics_indexed_bytes",
            });
        }
        if range.start > range.end || range.end > byte_length {
            return Err(CoreError::InvalidDomainValue {
                field: "line_metrics_range",
            });
        }
        if range.start == range.end {
            return Ok(LineMetrics::default());
        }

        let start = usize::try_from(range.start).map_err(|_| CoreError::InvalidDomainValue {
            field: "line_metrics_range",
        })?;
        let end = usize::try_from(range.end).map_err(|_| CoreError::InvalidDomainValue {
            field: "line_metrics_range",
        })?;
        let first_boundary = self
            .boundaries
            .partition_point(|boundary| *boundary <= range.start);
        let final_boundary = self
            .boundaries
            .partition_point(|boundary| *boundary <= range.end);
        let mut terminator_count = u64::try_from(final_boundary - first_boundary)
            .map_err(|_| line_limit_overflow("line_count"))?;

        if range.end == byte_length && !is_terminator_byte(bytes[end - 1]) {
            terminator_count =
                terminator_count
                    .checked_sub(1)
                    .ok_or(CoreError::InvalidDomainValue {
                        field: "line_metrics_index",
                    })?;
        }
        if range.end < byte_length && bytes[end - 1] == b'\r' && bytes[end] == b'\n' {
            terminator_count = terminator_count
                .checked_add(1)
                .ok_or_else(|| line_limit_overflow("line_count"))?;
        }

        Ok(LineMetrics {
            byte_count: range.end - range.start,
            terminator_count,
            first_byte: Some(bytes[start]),
            last_byte: Some(bytes[end - 1]),
        })
    }

    /// Returns the number of logical lines.
    #[must_use]
    pub fn line_count(&self) -> u64 {
        u64::try_from(self.boundaries.len()).unwrap_or(u64::MAX)
    }

    /// Returns the byte offset before a one-based line number.
    #[must_use]
    pub fn line_start(&self, line: u64) -> Option<u64> {
        if line == 0 || line > self.line_count() {
            return None;
        }
        match line {
            1 => Some(0),
            2.. => usize::try_from(line - 2)
                .ok()
                .and_then(|index| self.boundaries.get(index))
                .copied(),
            _ => None,
        }
    }

    /// Returns the exclusive byte end of a one-based line, including its terminator.
    #[must_use]
    pub fn line_end(&self, line: u64) -> Option<u64> {
        line.checked_sub(1)
            .and_then(|index| usize::try_from(index).ok())
            .and_then(|index| self.boundaries.get(index))
            .copied()
    }

    /// Returns the exact bytes used by the compact boundary representation.
    #[must_use]
    pub fn memory_bytes(&self) -> u64 {
        self.line_count().saturating_mul(8)
    }

    fn indexed_byte_length(&self) -> u64 {
        self.boundaries.last().copied().unwrap_or(0)
    }
}

/// Allocation-free metrics for a byte sequence under srcmv line semantics.
///
/// LF, CRLF, and lone CR each terminate one logical line. A nonempty suffix that
/// lacks a terminator is one line. Empty bytes have zero lines, and a trailing
/// terminator does not create a phantom line. Values can be composed in byte order
/// without losing CRLF sequences that cross chunk boundaries.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LineMetrics {
    byte_count: u64,
    terminator_count: u64,
    first_byte: Option<u8>,
    last_byte: Option<u8>,
}

impl LineMetrics {
    /// Scans one byte slice and returns its standalone line metrics.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::ResourceLimitExceeded`] if the byte length cannot be
    /// represented by the metrics type.
    pub fn try_from_bytes(bytes: &[u8]) -> Result<Self, CoreError> {
        let byte_count =
            u64::try_from(bytes.len()).map_err(|_| line_limit_overflow("line_metric_bytes"))?;
        if bytes.is_empty() {
            return Ok(Self::default());
        }

        let mut terminator_count = 0_u64;
        let mut offset = 0;
        while next_terminator_end(bytes, &mut offset).is_some() {
            terminator_count = terminator_count
                .checked_add(1)
                .ok_or_else(|| line_limit_overflow("line_count"))?;
        }
        Ok(Self {
            byte_count,
            terminator_count,
            first_byte: bytes.first().copied(),
            last_byte: bytes.last().copied(),
        })
    }

    /// Returns the number of bytes summarized by this value.
    #[must_use]
    pub const fn byte_count(self) -> u64 {
        self.byte_count
    }

    /// Returns the number of logical lines summarized by this value.
    #[must_use]
    pub fn line_count(self) -> u64 {
        let unterminated_suffix = self.last_byte.is_some_and(|byte| !is_terminator_byte(byte));
        self.terminator_count
            .saturating_add(u64::from(unterminated_suffix))
    }

    /// Appends a precomputed following summary in byte order.
    ///
    /// Empty summaries are identities. When this value ends in CR and `following`
    /// begins in LF, the two standalone terminators are merged into one CRLF
    /// terminator.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::ResourceLimitExceeded`] if the composed byte or line
    /// count cannot be represented. The receiver is unchanged on error.
    pub fn try_append(&mut self, following: Self) -> Result<(), CoreError> {
        if following.byte_count == 0 {
            return Ok(());
        }
        if self.byte_count == 0 {
            *self = following;
            return Ok(());
        }

        let byte_count = self
            .byte_count
            .checked_add(following.byte_count)
            .ok_or_else(|| line_limit_overflow("line_metric_bytes"))?;
        let following_terminators =
            if self.last_byte == Some(b'\r') && following.first_byte == Some(b'\n') {
                following
                    .terminator_count
                    .checked_sub(1)
                    .ok_or(CoreError::InvalidDomainValue {
                        field: "line_metrics_summary",
                    })?
            } else {
                following.terminator_count
            };
        let terminator_count = self
            .terminator_count
            .checked_add(following_terminators)
            .ok_or_else(|| line_limit_overflow("line_count"))?;

        self.byte_count = byte_count;
        self.terminator_count = terminator_count;
        self.last_byte = following.last_byte;
        Ok(())
    }

    /// Scans and appends a following byte chunk without allocating.
    ///
    /// Empty chunks are identities, and CRLF sequences split across calls are
    /// counted as one terminator.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::ResourceLimitExceeded`] if the chunk or composed
    /// metrics cannot be represented. The receiver is unchanged on error.
    pub fn try_append_bytes(&mut self, bytes: &[u8]) -> Result<(), CoreError> {
        self.try_append(Self::try_from_bytes(bytes)?)
    }
}

fn next_terminator_end(bytes: &[u8], offset: &mut usize) -> Option<usize> {
    while *offset < bytes.len() {
        match bytes[*offset] {
            b'\r' if bytes.get(*offset + 1) == Some(&b'\n') => {
                *offset += 2;
                return Some(*offset);
            }
            b'\r' | b'\n' => {
                *offset += 1;
                return Some(*offset);
            }
            _ => *offset += 1,
        }
    }
    None
}

const fn is_terminator_byte(byte: u8) -> bool {
    matches!(byte, b'\r' | b'\n')
}

fn push_line_boundary(
    boundaries: &mut Vec<u64>,
    boundary: usize,
    maximum_lines: u64,
    maximum_memory_bytes: u64,
) -> Result<(), CoreError> {
    let next_line_count = u64::try_from(boundaries.len())
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    enforce_line_limit("line_count", next_line_count, maximum_lines)?;
    let next_memory = next_line_count.saturating_mul(8);
    enforce_line_limit("line_index_memory", next_memory, maximum_memory_bytes)?;
    let boundary = u64::try_from(boundary).map_err(|_| CoreError::ResourceLimitExceeded {
        resource: "snapshot_file_bytes",
        actual: u64::MAX,
        limit: u64::MAX,
    })?;
    boundaries.push(boundary);
    Ok(())
}

fn enforce_line_limit(resource: &'static str, actual: u64, limit: u64) -> Result<(), CoreError> {
    if actual > limit {
        return Err(CoreError::ResourceLimitExceeded {
            resource,
            actual,
            limit,
        });
    }
    Ok(())
}

const fn line_limit_overflow(resource: &'static str) -> CoreError {
    CoreError::ResourceLimitExceeded {
        resource,
        actual: u64::MAX,
        limit: u64::MAX - 1,
    }
}

#[cfg(test)]
mod tests {
    use super::{LineIndex, LineMetrics};
    use crate::{ByteRange, CoreError};

    fn index(bytes: &[u8]) -> LineIndex {
        LineIndex::from_bytes_with_limits(bytes, u64::MAX, u64::MAX)
            .expect("unlimited test index should build")
    }

    fn metrics(bytes: &[u8]) -> LineMetrics {
        LineMetrics::try_from_bytes(bytes).expect("bounded test bytes should be representable")
    }

    #[test]
    fn line_index_should_handle_empty_and_unterminated_bytes() {
        let empty = index(b"");
        let unterminated = index(b"abc");

        assert_eq!(empty.line_count(), 0);
        assert_eq!(unterminated.line_count(), 1);
        assert_eq!(unterminated.line_start(1), Some(0));
        assert_eq!(unterminated.line_end(1), Some(3));
        assert_eq!(unterminated.line_start(2), None);
    }

    #[test]
    fn line_index_should_recognize_lf_crlf_lone_cr_and_mixed_terminators() {
        let line_index = index(b"a\nb\r\nc\rd");

        assert_eq!(line_index.line_count(), 4);
        assert_eq!(line_index.line_start(1), Some(0));
        assert_eq!(line_index.line_end(1), Some(2));
        assert_eq!(line_index.line_start(2), Some(2));
        assert_eq!(line_index.line_end(2), Some(5));
        assert_eq!(line_index.line_start(3), Some(5));
        assert_eq!(line_index.line_end(3), Some(7));
        assert_eq!(line_index.line_start(4), Some(7));
        assert_eq!(line_index.line_end(4), Some(8));
    }

    #[test]
    fn line_index_should_not_create_a_phantom_line_after_a_terminator() {
        assert_eq!(index(b"a\n").line_count(), 1);
        assert_eq!(index(b"a\r\n").line_count(), 1);
        assert_eq!(index(b"a\r").line_count(), 1);
    }

    #[test]
    fn line_index_should_treat_non_utf8_and_long_lines_as_bytes() {
        let mut bytes = vec![b'x'; 128 * 1024];
        bytes.extend_from_slice(&[0xff, b'\n']);
        let line_index = index(&bytes);

        assert_eq!(line_index.line_count(), 1);
        assert_eq!(line_index.line_end(1), Some(131_074));
        assert_eq!(line_index.memory_bytes(), 8);
    }

    #[test]
    fn line_index_should_enforce_line_and_memory_limits_before_a_boundary() {
        let line_error = LineIndex::from_bytes_with_limits(b"a\nb\n", 1, u64::MAX)
            .expect_err("second line should exceed limit");
        let memory_error = LineIndex::from_bytes_with_limits(b"a\n", u64::MAX, 7)
            .expect_err("one boundary needs eight bytes");

        assert!(matches!(
            line_error,
            CoreError::ResourceLimitExceeded {
                resource: "line_count",
                actual: 2,
                limit: 1
            }
        ));
        assert!(matches!(
            memory_error,
            CoreError::ResourceLimitExceeded {
                resource: "line_index_memory",
                actual: 8,
                limit: 7
            }
        ));
    }

    #[test]
    fn line_metrics_should_follow_the_byte_defined_contract() {
        let cases: &[(&[u8], u64)] = &[
            (b"", 0),
            (b"plain", 1),
            (b"plain\n", 1),
            (b"plain\r\n", 1),
            (b"plain\r", 1),
            (b"a\nb\r\nc\rd", 4),
            (b"\r\r", 2),
            (b"\n\n", 2),
            (b"a\nunterminated", 2),
            (&[0, 0xff, b'\r', b'\n', 0x80], 2),
        ];

        for (bytes, expected_lines) in cases {
            let actual = metrics(bytes);
            assert_eq!(actual.byte_count(), bytes.len() as u64, "bytes: {bytes:?}");
            assert_eq!(actual.line_count(), *expected_lines, "bytes: {bytes:?}");
        }
    }

    #[test]
    fn indexed_ranges_should_treat_crlf_cuts_as_standalone_bytes() {
        let bytes = b"a\r\nb";
        let line_index = index(bytes);

        let before_lf = line_index
            .metrics_for_range(bytes, ByteRange { start: 0, end: 2 })
            .expect("range ending after CR should be valid");
        let from_lf = line_index
            .metrics_for_range(bytes, ByteRange { start: 2, end: 3 })
            .expect("range starting at LF should be valid");
        let complete_crlf = line_index
            .metrics_for_range(bytes, ByteRange { start: 1, end: 3 })
            .expect("complete CRLF range should be valid");

        assert_eq!(before_lf.line_count(), 1);
        assert_eq!(from_lf.line_count(), 1);
        assert_eq!(complete_crlf.line_count(), 1);
    }

    #[test]
    fn indexed_ranges_should_reject_invalid_ranges_and_wrong_byte_lengths() {
        let bytes = b"a\r\nb";
        let line_index = index(bytes);

        let reversed = line_index
            .metrics_for_range(bytes, ByteRange { start: 3, end: 2 })
            .expect_err("reversed range should fail");
        let past_end = line_index
            .metrics_for_range(bytes, ByteRange { start: 0, end: 5 })
            .expect_err("range beyond bytes should fail");
        let wrong_bytes = line_index
            .metrics_for_range(b"short", ByteRange { start: 0, end: 1 })
            .expect_err("different byte length should fail");

        assert!(matches!(
            reversed,
            CoreError::InvalidDomainValue {
                field: "line_metrics_range"
            }
        ));
        assert!(matches!(
            past_end,
            CoreError::InvalidDomainValue {
                field: "line_metrics_range"
            }
        ));
        assert!(matches!(
            wrong_bytes,
            CoreError::InvalidDomainValue {
                field: "line_metrics_indexed_bytes"
            }
        ));
    }

    #[test]
    fn streaming_bytes_should_join_crlf_across_empty_chunks() {
        let mut actual = LineMetrics::default();
        actual
            .try_append_bytes(b"left\r")
            .expect("first chunk should append");
        actual
            .try_append_bytes(b"")
            .expect("empty chunk should append");
        actual
            .try_append_bytes(b"\nright")
            .expect("final chunk should append");

        assert_eq!(actual, metrics(b"left\r\nright"));
    }

    #[test]
    fn streaming_summary_should_report_overflow_without_mutation() {
        let mut actual = LineMetrics {
            byte_count: u64::MAX,
            terminator_count: u64::MAX,
            first_byte: Some(b'\r'),
            last_byte: Some(b'\r'),
        };
        let before = actual;

        let error = actual
            .try_append(metrics(b"\n"))
            .expect_err("one more byte should overflow");

        assert!(matches!(
            error,
            CoreError::ResourceLimitExceeded {
                resource: "line_metric_bytes",
                actual: u64::MAX,
                limit
            } if limit == u64::MAX - 1
        ));
        assert_eq!(actual, before);
    }
}
