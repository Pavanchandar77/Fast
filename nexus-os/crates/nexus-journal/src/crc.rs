//! ISO-HDLC CRC-32 (the PNG / IEEE polynomial). No extra crate.

use std::sync::OnceLock;

fn table() -> &'static [u32; 256] {
    static TABLE: OnceLock<[u32; 256]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut out = [0u32; 256];
        let mut i = 0;
        while i < 256 {
            let mut c = i as u32;
            let mut bit = 0;
            while bit < 8 {
                if c & 1 == 1 {
                    c = 0xEDB8_8320 ^ (c >> 1);
                } else {
                    c >>= 1;
                }
                bit += 1;
            }
            out[i] = c;
            i += 1;
        }
        out
    })
}

pub fn crc32(data: &[u8]) -> u32 {
    let tab = table();
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        let idx = ((crc ^ u32::from(byte)) & 0xFF) as usize;
        crc = tab[idx] ^ (crc >> 8);
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::crc32;

    #[test]
    fn iso_hdlc_check_value() {
        // The well-known check value for "123456789".
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }
}
