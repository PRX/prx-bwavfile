use crate::fourcc::FourCC;

/// `cart` chunk PostTimer entry.
///
/// Each `Cart` has exactly 8 of these. Unused entries should have a
/// zero `usage` FourCC.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct CartTimer {
    /// 4-character usage code identifying what this timer marks
    /// (e.g. `b"AUDs"`, `b"INT1"`, `b"EOD "`). Zero FourCC means unused.
    pub usage: FourCC,

    /// Sample-offset value associated with this timer.
    pub value: u32,
}

/// `cart` chunk record — broadcast-automation cart metadata
/// (AES46-2002, "Audio cart chunk for AES46-2002 cartridge labels").
///
/// The chunk carries title/artist/category metadata, scheduling
/// timestamps, level reference, post-roll timer marks, and free-form
/// tag text used by broadcast automation systems for scheduling and
/// playback of audio carts.
///
/// # Layout
///
/// The chunk has a fixed 2048-byte base for `Version >= "0101"` plus a
/// variable-length `tag_text` trailer. The legacy `"0000"` version
/// omits the 1024-byte URL field (1024-byte base instead of 2048).
///
/// All ASCII string fields are NUL-padded within their fixed slot. The
/// 276-byte `reserved` blob round-trips byte-exact for forward
/// compatibility with vendor extensions; it is **not** decoded as a
/// string (the AES46-2002 specification deliberately permits arbitrary
/// bytes here).
///
/// `level_reference` is **signed** (`i32`) per AES46-2002 §5.2.16, in
/// units of dB × 100 relative to digital full scale.
///
/// All multi-byte numeric fields are little-endian.
///
/// # Resources
/// - [AES46-2002 Cart Chunk overview](http://www.cartchunk.org/)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cart {
    /// Cart chunk version, 4 ASCII characters (e.g. `"0101"`).
    /// `"0000"` is a legacy format that omits the URL field.
    pub version: String,

    /// Cart title (max 64 ASCII chars).
    pub title: String,

    /// Performer / artist (max 64 ASCII chars).
    pub artist: String,

    /// Cut identifier (max 64 ASCII chars).
    pub cut_id: String,

    /// Client identifier (max 64 ASCII chars).
    pub client_id: String,

    /// Category (max 64 ASCII chars).
    pub category: String,

    /// Classification (max 64 ASCII chars).
    pub classification: String,

    /// Out cue text (max 64 ASCII chars).
    pub out_cue: String,

    /// Start date (10 ASCII chars, typically `YYYY-MM-DD`).
    pub start_date: String,

    /// Start time (8 ASCII chars, typically `HH:MM:SS`).
    pub start_time: String,

    /// End date (10 ASCII chars, typically `YYYY-MM-DD`).
    pub end_date: String,

    /// End time (8 ASCII chars, typically `HH:MM:SS`).
    pub end_time: String,

    /// Producer application identifier (max 64 ASCII chars).
    pub producer_app_id: String,

    /// Producer application version (max 64 ASCII chars).
    pub producer_app_version: String,

    /// User-defined field (max 64 ASCII chars).
    pub user_def: String,

    /// Reference level, signed, in dB × 100 relative to digital full scale.
    pub level_reference: i32,

    /// Eight post-roll timer entries.
    pub post_timers: [CartTimer; 8],

    /// 276 reserved bytes that must round-trip byte-exact.
    pub reserved: [u8; 276],

    /// URL (max 1024 ASCII chars). Absent (empty string written/read)
    /// for legacy `"0000"` version.
    pub url: String,

    /// Variable-length free-form tag text. Total cart chunk size is
    /// `2048 + tag_text.len()` (or `1024 + tag_text.len()` for `"0000"`).
    /// No NUL padding.
    pub tag_text: String,
}

impl Default for Cart {
    fn default() -> Self {
        Cart {
            version: "0101".to_string(),
            title: String::new(),
            artist: String::new(),
            cut_id: String::new(),
            client_id: String::new(),
            category: String::new(),
            classification: String::new(),
            out_cue: String::new(),
            start_date: String::new(),
            start_time: String::new(),
            end_date: String::new(),
            end_time: String::new(),
            producer_app_id: String::new(),
            producer_app_version: String::new(),
            user_def: String::new(),
            level_reference: 0,
            post_timers: <[CartTimer; 8]>::default(),
            reserved: [0u8; 276],
            url: String::new(),
            tag_text: String::new(),
        }
    }
}

impl Cart {
    /// True if this is a legacy `"0000"` cart with no URL field.
    pub fn is_legacy_v0(&self) -> bool {
        self.version == "0000"
    }

    /// Size in bytes of the fixed (non-tag-text) portion of the cart
    /// chunk for this cart's version.
    ///
    /// Returns `1024` for `"0000"`, `2048` for everything else.
    pub fn fixed_size(&self) -> u64 {
        if self.is_legacy_v0() {
            1024
        } else {
            2048
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunks::{ReadBWaveChunks, WriteBWaveChunks};
    use crate::fourcc::FourCC;
    use std::io::{Cursor, Seek, SeekFrom};

    fn round_trip(cart: Cart) {
        let mut buf = Cursor::new(Vec::<u8>::new());
        buf.write_cart(&cart).unwrap();
        let total_size = cart.fixed_size() as usize + cart.tag_text.len();
        assert_eq!(
            buf.get_ref().len(),
            total_size,
            "cart chunk content size mismatch"
        );
        buf.seek(SeekFrom::Start(0)).unwrap();
        let parsed = buf.read_cart(total_size as u64).unwrap();
        assert_eq!(parsed, cart);
    }

    #[test]
    fn round_trip_default() {
        round_trip(Cart::default());
    }

    #[test]
    fn round_trip_fully_populated() {
        let cart = Cart {
            version: "0101".to_string(),
            title: "Test Cart".to_string(),
            artist: "Test Artist".to_string(),
            cut_id: "CUT-12345".to_string(),
            client_id: "CLIENT-X".to_string(),
            category: "NEWS".to_string(),
            classification: "PROMO".to_string(),
            out_cue: "...thanks for listening".to_string(),
            start_date: "2026-05-01".to_string(),
            start_time: "12:00:00".to_string(),
            end_date: "2026-12-31".to_string(),
            end_time: "23:59:59".to_string(),
            producer_app_id: "bwavfile".to_string(),
            producer_app_version: "0.0.0".to_string(),
            user_def: "user defined data".to_string(),
            level_reference: 8195,
            post_timers: [
                CartTimer {
                    usage: FourCC::make(b"AUDs"),
                    value: 0,
                },
                CartTimer {
                    usage: FourCC::make(b"INT1"),
                    value: 1234,
                },
                CartTimer {
                    usage: FourCC::make(b"INT2"),
                    value: 5678,
                },
                CartTimer {
                    usage: FourCC::make(b"SEC1"),
                    value: 90123,
                },
                CartTimer {
                    usage: FourCC::make(b"EOD "),
                    value: u32::MAX,
                },
                CartTimer::default(),
                CartTimer::default(),
                CartTimer::default(),
            ],
            reserved: [0u8; 276],
            url: "https://example.com/cart".to_string(),
            tag_text: "Free-form tag text\r\nMultiple lines OK\r\n".to_string(),
        };
        round_trip(cart);
    }

    #[test]
    fn level_reference_signed_extremes() {
        for value in [i32::MIN, -1, 0, 1, i32::MAX] {
            round_trip(Cart {
                level_reference: value,
                ..Cart::default()
            });
        }
    }

    #[test]
    fn reserved_bytes_round_trip_verbatim() {
        // Fill reserved with non-ASCII pattern to verify byte-exact
        // round-trip — the 276 reserved bytes are preserved as raw
        // bytes (not decoded as a string), so vendor extensions that
        // store arbitrary bytes there survive a read+write cycle.
        let mut reserved = [0u8; 276];
        for (i, byte) in reserved.iter_mut().enumerate() {
            *byte = (i as u8).wrapping_mul(7).wrapping_add(0x80);
        }
        round_trip(Cart {
            reserved,
            ..Cart::default()
        });
    }

    #[test]
    fn tag_text_empty() {
        let cart = Cart::default();
        assert!(cart.tag_text.is_empty());
        round_trip(cart);
    }

    #[test]
    fn tag_text_odd_length() {
        // Odd-length tag_text triggers the RIFF pad-byte path in the
        // outer chunk writer; here we just verify the cart trailer
        // itself round-trips at odd length.
        round_trip(Cart {
            tag_text: "x".repeat(7),
            ..Cart::default()
        });
    }

    #[test]
    fn tag_text_long() {
        round_trip(Cart {
            tag_text: "y".repeat(8192),
            ..Cart::default()
        });
    }

    #[test]
    fn legacy_v0000_omits_url() {
        let cart = Cart {
            version: "0000".to_string(),
            // URL field is absent in v0000; default url is already empty.
            ..Cart::default()
        };
        assert_eq!(cart.fixed_size(), 1024);
        round_trip(cart);
    }
}
