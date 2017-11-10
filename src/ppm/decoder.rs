use std::io::Read;
use std::io::BufReader;
use std::ascii::AsciiExt;

use color::{ColorType};
use image::{DecodingResult, ImageDecoder, ImageResult, ImageError};
extern crate byteorder;
use self::byteorder::{BigEndian, ByteOrder};

enum DecodeStrategy {
    Bytes,
    Ascii,
}

/// PPM decoder
pub struct PPMDecoder<R> {
    reader: BufReader<R>,
    width: u32,
    height: u32,
    maxwhite: u32,
    depth: u32,
    decoder: DecodeStrategy,
}

impl<R: Read> PPMDecoder<R> {
    /// Create a new decoder that decodes from the stream ```r```
    pub fn new(read: R) -> ImageResult<PPMDecoder<R>> {
        let mut buf = BufReader::new(read);
        let mut magic: [u8; 2] = [0, 0];
        try!(buf.read_exact(&mut magic[..])); // Skip magic constant
        if magic[0] != b'P' {
            return Err(ImageError::FormatError("Expected magic constant for ppm, P3 or P6".to_string()));
        }

        let decoder = match magic[1] {
            b'3' => DecodeStrategy::Ascii,
            b'6' => DecodeStrategy::Bytes,
            _ => return Err(ImageError::FormatError("Expected magic constant for ppm, P3 or P6".to_string())),
        };

        let width = try!(PPMDecoder::read_next_u32(&mut buf));
        let height = try!(PPMDecoder::read_next_u32(&mut buf));
        let maxwhite = try!(PPMDecoder::read_next_u32(&mut buf));

        if !(maxwhite <= u16::max_value() as u32) {
            return Err(ImageError::FormatError("Image maxval is not less or equal to 65535".to_string()))
        }

        Ok(PPMDecoder {
            reader: buf,
            width: width,
            height: height,
            maxwhite: maxwhite,
            depth: 3,
            decoder: decoder,
        })
    }

    /// Reads a string as well as a single whitespace after it, ignoring comments
    fn read_next_string(reader: &mut BufReader<R>) -> ImageResult<String> {
        let mut bytes = Vec::new();

        // pair input bytes with a bool mask to remove comments
        let mark_comments = reader
            .bytes()
            .scan(true, |partof, read| {
                let byte = match read {
                    Err(err) => return Some((*partof, Err(err))),
                    Ok(byte) => byte,
                };
                let cur_enabled = *partof && byte != b'#';
                let next_enabled = cur_enabled || (byte == b'\r' || byte == b'\n');
                *partof = next_enabled;
                return Some((cur_enabled, Ok(byte)));
            });

        for (_, byte) in mark_comments.filter(|ref e| e.0) {
            match byte {
                Ok(b'\t') | Ok(b'\n') | Ok(b'\x0b') | Ok(b'\x0c') | Ok(b'\r') | Ok(b' ') => {
                    if !bytes.is_empty() {
                        break // We're done as we already have some content
                    }
                },
                Ok(byte) => {
                    bytes.push(byte);
                },
                Err(_) => break,
            }
        }

        if bytes.is_empty() {
            return Err(ImageError::FormatError("Unexpected eof".to_string()))
        }

        if !bytes.as_slice().is_ascii() {
            return Err(ImageError::FormatError("Non ascii character in preamble".to_string()))
        }

        String::from_utf8(bytes).map_err(|_| ImageError::FormatError("Couldn't read preamble".to_string()))
    }

    fn read_next_u32(reader: &mut BufReader<R>) -> ImageResult<u32> {
        let s = try!(PPMDecoder::read_next_string(reader));
        s.parse::<u32>().map_err(|_| ImageError::FormatError("Invalid number in preamble".to_string()))
    }
}

impl<R: Read> ImageDecoder for PPMDecoder<R> {
    fn dimensions(&mut self) -> ImageResult<(u32, u32)> {
        Ok((self.width, self.height))
    }

    fn colortype(&mut self) -> ImageResult<ColorType> {
        match (self.bytewidth(), self.components()) {
            (1, 1) => Ok(ColorType::Gray(8)),
            (2, 1) => Ok(ColorType::Gray(16)),
            (1, 3) => Ok(ColorType::RGB(8)),
            (2, 3) => Ok(ColorType::RGB(16)),
            _ => Err(ImageError::FormatError("Don't know how to decode PPM with more than 16 bits".to_string())),
        }
    }

    fn row_len(&mut self) -> ImageResult<usize> {
        Ok((self.width*self.components()*self.bytewidth()) as usize)
    }

    fn read_scanline(&mut self, _buf: &mut [u8]) -> ImageResult<u32> {
        unimplemented!();
    }

    fn read_image(&mut self) -> ImageResult<DecodingResult> {
        let pixelcount = self.width
            .checked_mul(self.height)
            .and_then(|v| v.checked_mul(self.components()));

        let pixelcount = match pixelcount {
            Some(v) => v,
            _ => return Err(ImageError::DimensionError),
        };

        self.read(pixelcount)
    }
}

impl<R: Read> PPMDecoder<R> {
    fn bytewidth(&self) -> u32 {
        if self.maxwhite < 256 { 1 } else { 2 }
    }

    fn components(&self) -> u32 {
        self.depth
    }

    fn read(&mut self, count: u32) -> ImageResult<DecodingResult> {
        if self.bytewidth() == 1 {
            let mut data = vec![0 as u8; count as usize];
            self.read_u8(&mut data)?;
            Ok(DecodingResult::U8(data))
        } else if self.bytewidth() == 2 {
            let mut data = vec![0 as u16; count as usize];
            self.read_u16(&mut data)?;
            Ok(DecodingResult::U16(data))
        } else {
            Err(ImageError::FormatError("Invalid sample bitwidth".to_string()))
        }
    }

    fn read_u8(&mut self, mut buffer: &mut [u8]) -> ImageResult<()> {
        match self.decoder {
            DecodeStrategy::Bytes => match self.reader.read_exact(&mut buffer) {
                    Ok(_) => Ok(()),
                    Err(e) => Err(ImageError::IoError(e)),
                },
            DecodeStrategy::Ascii => {
                    self.read_ascii::<u8>(&mut buffer)
                }
        }
    }

    fn read_u16(&mut self, mut buffer: &mut [u16]) -> ImageResult<()> {
        match self.decoder {
            DecodeStrategy::Bytes => {
                    let mut bytebuffer = vec![0 as u8; buffer.len() * 2];
                    match self.reader.read_exact(&mut bytebuffer) {
                        Err(e) => return Err(ImageError::IoError(e)),
                        Ok(_) => {},
                    }
                    BigEndian::read_u16_into(&mut bytebuffer, &mut buffer);
                    Ok(())
                },
            DecodeStrategy::Ascii => {
                    self.read_ascii::<u16>(&mut buffer)
                }
        }
    }

    fn read_ascii<Basic: FromSample>(&mut self, buffer: &mut [Basic::T]) -> ImageResult<()> {
        for pixel in buffer {
            let value = self.read_sample()?;
            *pixel = Basic::try_convert(value)?;
        }
        Ok(())
    }

    fn read_sample(&mut self) -> ImageResult<u32> {
        let istoken = |v: &Result<u8, _>| match v {
                &Err(_) => false,
                &Ok(b'\t') | &Ok(b'\n') | &Ok(b'\x0b') | &Ok(b'\x0c') | &Ok(b'\r') | &Ok(b' ') => false,
                _ => true,
            };
        let token = (&mut self.reader).bytes()
            .skip_while(|v| !istoken(v))
            .take_while(&istoken)
            .collect::<Result<Vec<u8>, _>>()?;
        if !token.is_ascii() {
            return Err(ImageError::FormatError("Non ascii character where sample value was expected".to_string()))
        }
        let string = String::from_utf8(token).map_err(|_| ImageError::FormatError("Error parsing sample".to_string()))?;
        string.parse::<u32>().map_err(|_| ImageError::FormatError("Error parsing sample value".to_string()))
    }
}

trait FromSample {
    type T;
    fn try_convert(u32) -> ImageResult<Self::T>;
}

impl FromSample for u8 {
    type T = u8;
    fn try_convert(val: u32) -> ImageResult<Self::T> {
        if val > u8::max_value() as u32 {
            Err(ImageError::FormatError("Sample value outside of bounds".to_string()))
        } else {
            Ok(val as u8)
        }
    }
}

impl FromSample for u16 {
    type T = u16;
    fn try_convert(val: u32) -> ImageResult<Self::T> {
        if val > u16::max_value() as u32 {
            Err(ImageError::FormatError("Sample value outside of bounds".to_string()))
        } else {
            Ok(val as u16)
        }
    }
}

/// Tests parsing binary buffers were written based on and validated against `pamfile` from
/// netpbm (http://netpbm.sourceforge.net/).
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_form() {
        // Violates current specification (October 2016 ) but accepted by both netpbm and ImageMagick
        decode_minimal_image(&b"P61 1 255 123"[..]);
        decode_minimal_image(&b"P6 1 1 255 123"[..]);
        decode_minimal_image(&b"P6 1 1 255 123\xFF"[..]); // Too long should not be an issue
    }

    #[test]
    fn comment_in_token() {
        decode_minimal_image(&b"P6 1 1 2#comment\n55 123"[..]); // Terminating LF
        decode_minimal_image(&b"P6 1 1 2#comment\r55 123"[..]); // Terminating CR
        decode_minimal_image(&b"P6 1 1#comment\n 255 123"[..]); // Comment after token
        decode_minimal_image(&b"P6 1 1 #comment\n255 123"[..]); // Comment before token
        decode_minimal_image(&b"P6#comment\n 1 1 255 123"[..]); // Begin of header
        decode_minimal_image(&b"P6 1 1 255#comment\n 123"[..]); // End of header
    }

    #[test]
    fn whitespace() {
        decode_minimal_image(&b"P6\x091\x091\x09255\x09123"[..]); // TAB
        decode_minimal_image(&b"P6\x0a1\x0a1\x0a255\x0a123"[..]); // LF
        decode_minimal_image(&b"P6\x0b1\x0b1\x0b255\x0b123"[..]); // VT
        decode_minimal_image(&b"P6\x0c1\x0c1\x0c255\x0c123"[..]); // FF
        decode_minimal_image(&b"P6\x0d1\x0d1\x0d255\x0d123"[..]); // CR
        // Spaces tested before
        decode_minimal_image(&b"P61\x09\x0a\x0b\x0c\x0d1 255 123"[..]); // All whitespace, combined
    }

    #[test]
    fn ascii_encoded() {
        decode_minimal_image(&b"P31 1 255 49 50 51"[..]);
        assert!(PPMDecoder::new(&b"P31 1 65535 65535 65535 65535"[..]).unwrap()
            .read_image().is_ok()); // Maximum sample size
        decode_minimal_image(&b"P31 1 255  49 50 51"[..]); // Whitespace after header
        decode_minimal_image(&b"P31 1 255 49\n\t 50\r\x0b\x0c51"[..]); // All forms of whitespace
    }

    /// Tests for decoding error, assuming `encoded` is ppm encoding for the very simplistic image
    /// containing a single pixel with one byte values (1, 2, 3).
    fn decode_minimal_image(encoded: &[u8]) {
        let content = vec![49 as u8, 50, 51];
        let mut decoder = PPMDecoder::new(encoded).unwrap();

        assert_eq!(decoder.dimensions().unwrap(), (1, 1));
        assert_eq!(decoder.colortype().unwrap(), ColorType::RGB(8));
        assert_eq!(decoder.row_len().unwrap(), 3);
        assert_eq!(decoder.bytewidth(), 1);

        match decoder.read_image().unwrap() {
            DecodingResult::U8(image) => assert_eq!(image, content),
            _ => assert!(false),
        }
    }

    #[test]
    fn wrong_tag() {
        assert!(PPMDecoder::new(&b"P5 1 1 255 1"[..]).is_err());
    }

    #[test]
    fn invalid_characters() {
        assert!(PPMDecoder::new(&b"P6 1chars1 255 1"[..]).is_err()); // No text outside of comments
        assert!(PPMDecoder::new(&b"P6 1\xFF1 255 1"[..]).is_err()); // No invalid ascii chars
        assert!(PPMDecoder::new(&b"P6 0x01 1 255 1"[..]).is_err()); // Numbers only as decimal
    }

    /// These violate the narrow specification of ppm but are commonly supported in other programs.
    /// Fail fast and concise is important here as these might be received as input files.
    #[test]
    fn unsupported_extensions() {
        assert!(PPMDecoder::new(&b"P6 1 1 65536 1"[..]).is_err()); // No bitwidth above 16
    }
}
