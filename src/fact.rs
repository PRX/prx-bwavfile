/// `fact` chunk record.
///
/// Required for non-PCM data per [EBU Tech 3285 Supplement 1][s1]. The
/// `fact` chunk carries the total number of samples per channel after
/// decoding. For MPEG audio, this equals
/// `frame_count * samples_per_frame`.
///
/// ## RF64 note
///
/// `dwSampleLength` is `u32`. For files exceeding 2^32 samples, the
/// long sample count belongs in `ds64.factSampleLength` per ITU-R
/// BS.2088 / EBU Tech 3306. The current `WaveWriter` does not yet
/// route `fact` through `ds64`, so writing extreme-length non-PCM
/// streams correctly is not supported. In practice this is unreachable
/// for typical broadcast files, but a future RF64 enhancement may
/// extend `Fact` accordingly.
///
/// ## Resources
/// - [EBU Tech 3285 Supplement 1][s1]
///
/// [s1]: https://tech.ebu.ch/docs/tech/tech3285s1.pdf
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Fact {
    /// Number of samples per channel after decoding.
    pub sample_length: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunks::{ReadBWaveChunks, WriteBWaveChunks};
    use std::io::{Cursor, Seek, SeekFrom};

    fn round_trip(sample_length: u32) {
        let original = Fact { sample_length };
        let mut buf = Cursor::new(Vec::<u8>::new());
        buf.write_fact(&original).unwrap();
        // fact chunk content is exactly 4 bytes (a u32)
        assert_eq!(buf.get_ref().len(), 4);
        buf.seek(SeekFrom::Start(0)).unwrap();
        let parsed = buf.read_fact().unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn round_trip_zero() {
        round_trip(0);
    }

    #[test]
    fn round_trip_one() {
        round_trip(1);
    }

    #[test]
    fn round_trip_max() {
        round_trip(u32::MAX);
    }
}
