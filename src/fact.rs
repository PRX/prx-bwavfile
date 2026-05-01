/// `fact` chunk record.
///
/// Required for non-PCM data per [EBU Tech 3285 Supplement 1][s1]. The
/// `fact` chunk carries the total number of samples per channel after
/// decoding. For MPEG audio, this is `frame_count * samples_per_frame`.
///
/// ## RF64 note
///
/// `dwSampleLength` is `u32`. For files exceeding 2^32 samples, the
/// long sample count belongs in `ds64.factSampleLength` (per ITU-R
/// BS.2088 / EBU Tech 3306). The current `WaveWriter` does not yet
/// route `fact` through `ds64`, so very large MP2 files (> ~14 hours
/// at 44.1 kHz mono) cannot be written correctly. In practice, MP2
/// broadcast files run < 4 GB and stay well under the limit.
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
    use crate::wavereader::WaveReader;
    use crate::wavewriter::WaveWriter;
    use crate::WaveFmt;
    use std::io::{Cursor, Seek, SeekFrom};

    fn round_trip(sample_length: u32) {
        let original = Fact { sample_length };
        let mut buf = Cursor::new(Vec::<u8>::new());
        buf.write_fact(&original).unwrap();
        buf.seek(SeekFrom::Start(0)).unwrap();
        let parsed = buf.read_fact().unwrap();
        assert_eq!(parsed, original);
        // fact chunk content is exactly 4 bytes
        assert_eq!(buf.into_inner().len(), 4);
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

    #[test]
    fn round_trip_via_wave_writer() {
        let mut cursor = Cursor::new(Vec::<u8>::new());
        let format = WaveFmt::new_pcm_mono(48000, 24);
        let mut w = WaveWriter::new(&mut cursor, format).unwrap();
        w.write_fact(&Fact {
            sample_length: 12345,
        })
        .unwrap();
        // Also append a minimal data chunk so the file passes WaveReader's
        // fmt-and-data validation.
        let mut frame_writer = w.audio_frame_writer().unwrap();
        frame_writer.write_frames(&[0i32]).unwrap();
        frame_writer.end().unwrap();

        cursor.seek(SeekFrom::Start(0)).unwrap();
        let mut reader = WaveReader::new(cursor).unwrap();
        let parsed = reader.fact().unwrap().expect("fact chunk should be present");
        assert_eq!(parsed.sample_length, 12345);
    }

    #[test]
    fn no_fact_chunk_returns_none() {
        // pt_24bit.wav is plain PCM with no fact chunk.
        let mut reader = WaveReader::open("tests/media/pt_24bit.wav").unwrap();
        assert!(reader.fact().unwrap().is_none());
    }

    #[test]
    fn read_60315_wav_has_fact() {
        // 60315.wav is a real broadcast WAV with MP2 audio; fact is mandatory.
        let mut reader = WaveReader::open("tests/media/60315.wav").unwrap();
        let fact = reader.fact().unwrap().expect("60315.wav must have a fact chunk");
        assert!(fact.sample_length > 0);
    }
}
