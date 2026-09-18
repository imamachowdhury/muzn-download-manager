//! Deciding how a file is split before any connection exists. Pure arithmetic,
//! so it is property-tested here and never touched by I/O.

/// A segment is never planned smaller than this (1 MiB).
pub const MIN_SEGMENT_BYTES: u64 = 1024 * 1024;

/// Hard ceiling on connections per download, whatever the setting says.
pub const MAX_CONNECTIONS: u8 = 32;

/// One byte range of a download and how much of it has been written.
///
/// `end` is inclusive, like an HTTP `Range`. The next byte to fetch is
/// `start + downloaded`; the segment is done when that passes `end`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegmentState {
    /// Position in the plan; stable for the life of the download.
    pub idx: u32,
    /// First byte (absolute offset).
    pub start: u64,
    /// Last byte (absolute offset, inclusive).
    pub end: u64,
    /// Bytes already written, counted from `start`.
    pub downloaded: u64,
}

impl SegmentState {
    /// Bytes still to fetch.
    pub fn remaining(&self) -> u64 {
        (self.end + 1).saturating_sub(self.start + self.downloaded)
    }
    /// True once every byte up to `end` is written.
    pub fn is_done(&self) -> bool {
        self.remaining() == 0
    }
    /// Absolute offset of the next byte to fetch.
    pub fn next_offset(&self) -> u64 {
        self.start + self.downloaded
    }
}

/// Split `size` bytes into contiguous, near-equal segments.
///
/// Count = `min(max_connections, ceil(size / MIN_SEGMENT_BYTES))`, at least 1
/// and never above [`MAX_CONNECTIONS`]. A zero-size file plans nothing.
pub fn plan_segments(size: u64, max_connections: u8) -> Vec<SegmentState> {
    if size == 0 {
        return Vec::new();
    }
    let by_size = size.div_ceil(MIN_SEGMENT_BYTES).max(1);
    let wanted = u64::from(max_connections.clamp(1, MAX_CONNECTIONS));
    let count = by_size.min(wanted);
    let base = size / count;
    let extra = size % count; // the first `extra` segments get one more byte
    let mut out = Vec::with_capacity(count as usize);
    let mut start = 0u64;
    for idx in 0..count {
        let len = base + u64::from(idx < extra);
        out.push(SegmentState {
            idx: idx as u32,
            start,
            end: start + len - 1,
            downloaded: 0,
        });
        start += len;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn small_file_is_one_segment() {
        let s = plan_segments(500_000, 8);
        assert_eq!(s.len(), 1);
        assert_eq!((s[0].start, s[0].end), (0, 499_999));
    }

    #[test]
    fn segment_count_is_bounded_by_size_and_connections() {
        assert_eq!(plan_segments(3 * MIN_SEGMENT_BYTES, 8).len(), 3);
        assert_eq!(plan_segments(100 * MIN_SEGMENT_BYTES, 8).len(), 8);
        assert_eq!(
            plan_segments(100 * MIN_SEGMENT_BYTES, 0).len(),
            1,
            "0 connections means 1"
        );
        assert_eq!(
            plan_segments(100 * MIN_SEGMENT_BYTES, 64).len(),
            32,
            "capped at 32"
        );
    }

    #[test]
    fn zero_size_plans_nothing() {
        assert!(plan_segments(0, 8).is_empty());
    }

    #[test]
    fn remaining_and_done() {
        let mut s = SegmentState {
            idx: 0,
            start: 10,
            end: 19,
            downloaded: 0,
        };
        assert_eq!(s.remaining(), 10);
        assert!(!s.is_done());
        s.downloaded = 10;
        assert_eq!(s.remaining(), 0);
        assert!(s.is_done());
        assert_eq!(s.next_offset(), 20);
    }

    proptest! {
        #[test]
        fn segments_cover_the_file_exactly_once(size in 1u64..(200 * MIN_SEGMENT_BYTES), conns in 1u8..=32) {
            let s = plan_segments(size, conns);
            prop_assert_eq!(s[0].start, 0);
            prop_assert_eq!(s.last().unwrap().end, size - 1);
            for (i, w) in s.windows(2).enumerate() {
                prop_assert_eq!(w[0].idx as usize, i);
                prop_assert_eq!(w[0].end + 1, w[1].start, "contiguous");
            }
            let total: u64 = s.iter().map(|x| x.end - x.start + 1).sum();
            prop_assert_eq!(total, size);
            let (min, max) = s.iter().fold((u64::MAX, 0), |(lo, hi), x| {
                let n = x.end - x.start + 1;
                (lo.min(n), hi.max(n))
            });
            prop_assert!(max - min <= 1, "near-equal sizes: {} vs {}", min, max);
        }
    }
}
