use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::bext::Bext;
use crate::cart::Cart;
use crate::errors::Error;
use crate::fact::Fact;
use crate::fmt::WaveFmt;
use crate::mext::Mext;
use crate::mpeg::MpegInfo;
use crate::wavewriter::WaveWriter;

/// High-level builder for wrapping an MPEG-1 audio bitstream in a
/// Broadcast WAVE container.
///
/// This is the primary entry point for the production use case: take
/// an MP2 file plus optional broadcast metadata and produce a
/// BWF-compliant WAVE file containing the MP2 data plus the chunks
/// broadcast automation systems expect (`fmt` extended for MPEG,
/// `fact`, `mext`, `bext`, `cart`).
///
/// # Example
///
/// ```no_run
/// use bwavfile::{BroadcastMpegFile, Bext};
///
/// let bext = Bext {
///     description: "PRX broadcast cart".into(),
///     originator: "PRX".into(),
///     // ... other fields ...
/// #     originator_reference: String::new(),
/// #     origination_date: "2026-05-01".into(),
/// #     origination_time: "12:00:00".into(),
/// #     time_reference: 0,
/// #     version: 0,
/// #     umid: None,
/// #     loudness_value: None,
/// #     loudness_range: None,
/// #     max_true_peak_level: None,
/// #     max_momentary_loudness: None,
/// #     max_short_term_loudness: None,
/// #     coding_history: String::new(),  // empty → synthesized per EBU R98-1999
/// };
///
/// BroadcastMpegFile::from_path("input.mp2")?
///     .with_bext(bext)
///     .write_to_path("output.wav")?;
/// # Ok::<(), bwavfile::Error>(())
/// ```
///
/// # Chunk write order
///
/// Chunks are written in the order broadcast tools expect:
/// `fmt` → `fact` → `mext` → `bext` (if any) → `cart` (if any) → `data`.
///
/// # ID3v2 handling
///
/// If the MPEG source begins with an ID3v2 tag, that prefix is
/// stripped — broadcast WAVs carry metadata in `bext` and `cart`, not
/// ID3. The MPEG audio body (starting at the first MPEG sync byte) is
/// written verbatim to the `data` chunk.
///
/// # Coding history
///
/// If `bext.coding_history` is empty when [`write_to`](Self::write_to)
/// is called, a default value is synthesized in EBU R98-1999 format:
/// `A=MPEG{ver}L{layer},F={sample_rate},B={bit_rate_kbps},M={mode},T=prx-bwavfile\r\n`.
/// To override, set `coding_history` on the `Bext` before calling
/// [`with_bext`](Self::with_bext).
pub struct BroadcastMpegFile {
    info: MpegInfo,
    /// MPEG audio bytes with any ID3v2 prefix already stripped.
    mpeg_bytes: Vec<u8>,
    bext: Option<Bext>,
    cart: Option<Cart>,
}

impl BroadcastMpegFile {
    /// Construct from an in-memory MPEG byte buffer (with or without
    /// ID3v2 prefix).
    pub fn from_buffer(buf: &[u8]) -> Result<Self, Error> {
        let info = MpegInfo::from_buffer(buf)?;
        let start = info.id3v2_offset as usize;
        let mpeg_bytes = buf[start..].to_vec();
        Ok(Self {
            info,
            mpeg_bytes,
            bext: None,
            cart: None,
        })
    }

    /// Construct from a `Read + Seek` source. The reader is rewound
    /// to position 0 and fully buffered into memory.
    pub fn from_reader<R: Read + Seek>(reader: &mut R) -> Result<Self, Error> {
        reader.seek(SeekFrom::Start(0))?;
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf)?;
        Self::from_buffer(&buf)
    }

    /// Construct by reading an MPEG file from disk.
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let bytes = std::fs::read(path)?;
        Self::from_buffer(&bytes)
    }

    /// Attach a Broadcast-WAVE `bext` chunk.
    ///
    /// If the supplied `bext.coding_history` is empty, a default value
    /// is synthesized in EBU R98-1999 format at write time.
    pub fn with_bext(mut self, bext: Bext) -> Self {
        self.bext = Some(bext);
        self
    }

    /// Attach an AES46-2002 `cart` chunk.
    pub fn with_cart(mut self, cart: Cart) -> Self {
        self.cart = Some(cart);
        self
    }

    /// Parsed MPEG frame info from the source.
    pub fn info(&self) -> &MpegInfo {
        &self.info
    }

    /// MPEG audio bytes that will be written to the `data` chunk
    /// (with any ID3v2 prefix already stripped).
    pub fn mpeg_bytes(&self) -> &[u8] {
        &self.mpeg_bytes
    }

    /// Write the broadcast WAVE to a `Write + Seek` target.
    ///
    /// Consumes `self` because the underlying `WaveWriter` owns the
    /// writer, and re-using a `BroadcastMpegFile` to write multiple
    /// outputs is uncommon in practice — clone before calling if
    /// needed.
    pub fn write_to<W: Write + Seek>(self, writer: W) -> Result<(), Error> {
        let format = WaveFmt::new_mpeg1(&self.info);
        // Skip the 96-byte ds64 JUNK reservation. The reservation only
        // matters if the file's form_length (sum of all chunks) might
        // exceed 4 GiB and trigger RF64 promotion. For BroadcastMpegFile:
        //   - WaveWriter::write_data_raw asserts the data chunk itself
        //     is < 4 GiB, capping the dominant contributor.
        //   - The metadata chunks (fmt, fact, mext, bext, cart) total at
        //     most a few KB unless cart.tag_text is unusually large.
        //   - 256 kbps MP2 at the 4 GiB data-chunk ceiling would be
        //     ~36 hours — well beyond any realistic broadcast cart.
        // So in practice form_length stays well under 4 GiB and ds64
        // is never needed.
        let mut w = WaveWriter::new_without_ds64_reservation(writer, format)?;

        // fact: total decoded sample count, mandatory for non-PCM
        w.write_fact(&Fact {
            sample_length: self.info.sample_length as u32,
        })?;

        // mext: MPEG-specific framing info
        w.write_mext(&Mext::from_mpeg_info(&self.info))?;

        // bext: synthesize coding_history if absent
        if let Some(mut bext) = self.bext {
            if bext.coding_history.is_empty() {
                bext.coding_history = default_coding_history(&self.info);
            }
            w.write_broadcast_metadata(&bext)?;
        }

        // cart: written as-is
        if let Some(cart) = self.cart {
            w.write_cart(&cart)?;
        }

        // data: raw MPEG bytes — bypass the elm1-aligned audio frame
        // writer (its 0x4000 alignment is correct for PCM, wrong for
        // codec data).
        w.write_data_raw(&self.mpeg_bytes)?;

        Ok(())
    }

    /// Write the broadcast WAVE to a path, using buffered I/O.
    pub fn write_to_path<P: AsRef<Path>>(self, path: P) -> Result<(), Error> {
        let f = File::create(path)?;
        let bw = BufWriter::new(f);
        self.write_to(bw)
    }
}

/// Synthesize an EBU R98-1999 coding-history line for an MPEG bitstream.
///
/// Format: `A=MPEG{version}L{layer},F={sample_rate},B={bit_rate_kbps},M={mode},T=prx-bwavfile\r\n`
fn default_coding_history(info: &MpegInfo) -> String {
    format!(
        "A=MPEG{}L{},F={},B={},M={},T=prx-bwavfile\r\n",
        info.version.r98_label(),
        info.layer.number(),
        info.sample_rate,
        info.bit_rate,
        info.channel_mode.r98_label(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wavereader::WaveReader;
    use std::io::Cursor;

    fn read_fixture(name: &str) -> Vec<u8> {
        std::fs::read(format!("tests/media/{}", name))
            .expect("fixture missing — run create_test_media.sh")
    }

    #[test]
    fn end_to_end_test_mp2_to_bwf() {
        let mut cursor = Cursor::new(Vec::<u8>::new());
        BroadcastMpegFile::from_path("tests/media/test.mp2")
            .unwrap()
            .write_to(&mut cursor)
            .unwrap();

        cursor.seek(SeekFrom::Start(0)).unwrap();
        let mut reader = WaveReader::new(&mut cursor).unwrap();

        // fmt: tag 0x0050, MPEG-1 extension populated
        let format = reader.format().unwrap();
        assert_eq!(format.tag, 0x0050);
        assert!(format.extended_format.is_none());
        let mpeg1 = format.mpeg1_extension().expect("MPEG-1 extension required");
        assert_eq!(mpeg1.head_layer, 2, "Layer II → head_layer=2");
        assert_eq!(mpeg1.head_bit_rate, 256_000, "256 kbps in bps");
        assert!(
            mpeg1.head_mode == 1 || mpeg1.head_mode == 2,
            "stereo or joint-stereo"
        );

        // fact present and consistent with frame count
        let fact = reader.fact().unwrap().expect("fact chunk required");
        assert!(fact.sample_length > 0);

        // mext present, homogeneous bit set (test.mp2 is CBR)
        let mext = reader.mext().unwrap().expect("mext chunk required");
        assert!(mext.sound_information & Mext::SOUND_HOMOGENEOUS != 0);
        assert!(mext.frame_size > 0);

        // bext is None unless we attached one
        assert!(reader.broadcast_extension().unwrap().is_none());

        // cart is None unless we attached one
        assert!(reader.cart().unwrap().is_none());
    }

    #[test]
    fn data_chunk_bytes_match_source() {
        let mut cursor = Cursor::new(Vec::<u8>::new());
        let bmf = BroadcastMpegFile::from_path("tests/media/test.mp2").unwrap();
        let expected_mpeg = bmf.mpeg_bytes().to_vec();
        bmf.write_to(&mut cursor).unwrap();

        // Pull the data chunk's bytes back out via the RIFF parser.
        use crate::fourcc::DATA_SIG;
        use crate::parser::Parser;

        cursor.seek(SeekFrom::Start(0)).unwrap();
        let chunks = Parser::make(&mut cursor)
            .unwrap()
            .into_chunk_list()
            .unwrap();
        let data_chunk = chunks
            .iter()
            .find(|c| c.signature == DATA_SIG)
            .expect("data chunk required");

        cursor.seek(SeekFrom::Start(data_chunk.start)).unwrap();
        let mut data_buf = vec![0u8; data_chunk.length as usize];
        cursor.read_exact(&mut data_buf).unwrap();

        assert_eq!(
            data_buf, expected_mpeg,
            "data chunk bytes must equal the source MPEG body verbatim"
        );
    }

    #[test]
    fn id3v2_prefix_is_stripped() {
        // test-id3.mp2 has an ID3v2 prefix; the BWF data chunk should
        // contain only the MPEG body.
        let bmf = BroadcastMpegFile::from_path("tests/media/test-id3.mp2").unwrap();
        let id3v2_offset = bmf.info().id3v2_offset;
        assert!(id3v2_offset > 0, "test-id3.mp2 should have an ID3v2 prefix");

        let source = read_fixture("test-id3.mp2");
        let expected_body = &source[id3v2_offset as usize..];
        assert_eq!(bmf.mpeg_bytes(), expected_body);
    }

    #[test]
    fn synthesizes_coding_history_when_bext_is_empty() {
        let bext = Bext {
            description: "Test".into(),
            originator: "PRX".into(),
            originator_reference: String::new(),
            origination_date: "2026-05-01".into(),
            origination_time: "12:00:00".into(),
            time_reference: 0,
            version: 0,
            umid: None,
            loudness_value: None,
            loudness_range: None,
            max_true_peak_level: None,
            max_momentary_loudness: None,
            max_short_term_loudness: None,
            coding_history: String::new(),
        };

        let mut cursor = Cursor::new(Vec::<u8>::new());
        BroadcastMpegFile::from_path("tests/media/test.mp2")
            .unwrap()
            .with_bext(bext)
            .write_to(&mut cursor)
            .unwrap();

        cursor.seek(SeekFrom::Start(0)).unwrap();
        let mut reader = WaveReader::new(&mut cursor).unwrap();
        let parsed = reader.broadcast_extension().unwrap().unwrap();
        // Synthesized: starts with "A=MPEG1L2," and ends with PRX tool tag.
        assert!(
            parsed.coding_history.starts_with("A=MPEG1L2,"),
            "expected EBU R98 prefix, got: {}",
            parsed.coding_history
        );
        assert!(
            parsed.coding_history.contains("T=prx-bwavfile"),
            "expected PRX tool tag, got: {}",
            parsed.coding_history
        );
    }

    #[test]
    fn preserves_user_supplied_coding_history() {
        let custom_history = "A=PCM,F=48000,W=24,M=stereo,T=test\r\n".to_string();
        let bext = Bext {
            description: "Test".into(),
            originator: "PRX".into(),
            originator_reference: String::new(),
            origination_date: "2026-05-01".into(),
            origination_time: "12:00:00".into(),
            time_reference: 0,
            version: 0,
            umid: None,
            loudness_value: None,
            loudness_range: None,
            max_true_peak_level: None,
            max_momentary_loudness: None,
            max_short_term_loudness: None,
            coding_history: custom_history.clone(),
        };

        let mut cursor = Cursor::new(Vec::<u8>::new());
        BroadcastMpegFile::from_path("tests/media/test.mp2")
            .unwrap()
            .with_bext(bext)
            .write_to(&mut cursor)
            .unwrap();

        cursor.seek(SeekFrom::Start(0)).unwrap();
        let mut reader = WaveReader::new(&mut cursor).unwrap();
        let parsed = reader.broadcast_extension().unwrap().unwrap();
        assert_eq!(parsed.coding_history, custom_history);
    }

    #[test]
    fn full_metadata_roundtrip() {
        let bext = Bext {
            description: "Full metadata test".into(),
            originator: "PRX".into(),
            originator_reference: String::new(),
            origination_date: "2026-05-01".into(),
            origination_time: "12:00:00".into(),
            time_reference: 0,
            version: 0,
            umid: None,
            loudness_value: None,
            loudness_range: None,
            max_true_peak_level: None,
            max_momentary_loudness: None,
            max_short_term_loudness: None,
            coding_history: String::new(),
        };
        let cart = Cart {
            title: "End-to-end test".into(),
            artist: "PRX".into(),
            ..Cart::default()
        };

        let mut cursor = Cursor::new(Vec::<u8>::new());
        BroadcastMpegFile::from_path("tests/media/test.mp2")
            .unwrap()
            .with_bext(bext.clone())
            .with_cart(cart.clone())
            .write_to(&mut cursor)
            .unwrap();

        cursor.seek(SeekFrom::Start(0)).unwrap();
        let mut reader = WaveReader::new(&mut cursor).unwrap();

        assert!(reader.fact().unwrap().is_some());
        assert!(reader.mext().unwrap().is_some());

        let parsed_bext = reader.broadcast_extension().unwrap().unwrap();
        assert_eq!(parsed_bext.description, bext.description);

        let parsed_cart = reader.cart().unwrap().unwrap();
        assert_eq!(parsed_cart.title, cart.title);
        assert_eq!(parsed_cart.artist, cart.artist);
    }

    #[test]
    fn audio_frame_reader_refuses_mpeg_cleanly() {
        // The output BWF has tag=0x0050; AudioFrameReader can't decode
        // MPEG, but should return a clean error rather than panicking.
        let mut cursor = Cursor::new(Vec::<u8>::new());
        BroadcastMpegFile::from_path("tests/media/test.mp2")
            .unwrap()
            .write_to(&mut cursor)
            .unwrap();

        cursor.seek(SeekFrom::Start(0)).unwrap();
        let reader = WaveReader::new(&mut cursor).unwrap();
        let result = reader.audio_frame_reader();
        match result {
            Err(Error::UnsupportedAudioFormat { tag: 0x0050 }) => {}
            other => panic!("expected UnsupportedAudioFormat, got: {:?}", other),
        }
    }
}
