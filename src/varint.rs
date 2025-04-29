use std::{
    io,
    ops::{BitOr, Shr},
    slice,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sign {
    Positive,
    Negative,
}

impl Sign {
    pub fn is_positive(self) -> bool {
        matches!(self, Self::Positive)
    }
    pub fn is_negative(self) -> bool {
        matches!(self, Self::Negative)
    }

    pub fn into_neg_bit(self) -> bool {
        match self {
            Sign::Positive => false,
            Sign::Negative => true,
        }
    }

    pub fn from_neg_bit(bit: bool) -> Self {
        match bit {
            false => Sign::Positive,
            true => Sign::Negative,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum VarIntReadError {
    #[error(transparent)]
    IOError(#[from] io::Error),

    #[error("Read value is too big for the integer type")]
    ValueTooBig,

    #[error("Read invalid signed value")]
    InvalidSignedValue,
}

pub trait UnsignedInt: Copy + Shr<u32, Output = Self> + BitOr<Output = Self> {
    const ZERO: Self;

    fn is_zero(self) -> bool;

    fn from_u8_bits(val: u8) -> Self;
    fn into_u8_bits_trimmed(self) -> u8;

    fn checked_shl(self, shift: u32) -> Option<Self>;
}

pub trait SignedInt: Copy {
    type Unsigned: UnsignedInt;

    fn into_split_sign(self) -> (Self::Unsigned, Sign);
    fn from_split_sign(val: Self::Unsigned, sign: Sign) -> Option<Self>;
}

pub fn write_unsigned_varint<I: UnsignedInt, W: io::Write>(
    mut writer: W,
    mut value: I,
) -> io::Result<usize> {
    let mut more = true;

    let mut buf = [0u8; 16];
    let mut buf_len = 0;
    let mut written = 0;

    while more {
        let data = value.into_u8_bits_trimmed() & 0b01111111;
        value = value >> 7;

        more = !value.is_zero();
        let data = if more { data | 0b10000000 } else { data };

        if buf_len >= buf.len() {
            writer.write_all(&buf[..buf_len])?;
            buf_len = 0;
            written += buf_len;
        }

        buf[buf_len] = data;
        buf_len += 1;
    }

    if buf_len > 0 {
        writer.write_all(&buf[..buf_len])?;
        written += buf_len;
    }

    Ok(written)
}

pub fn write_signed_varint<I: SignedInt, W: io::Write>(writer: W, value: I) -> io::Result<usize> {
    let (value, sign) = value.into_split_sign();
    write_varint_with_sign(writer, value, sign)
}

pub fn write_varint_with_sign<I: UnsignedInt, W: io::Write>(
    mut writer: W,
    mut value: I,
    sign: Sign,
) -> io::Result<usize> {
    let mut more = true;
    let mut first = true;

    let mut buf = [0u8; 16];
    let mut buf_len = 0;
    let mut written = 0;

    while more {
        let (bits, mask) = if first {
            (6, 0b00111111)
        } else {
            (7, 0b01111111)
        };

        let data = value.into_u8_bits_trimmed() & mask;
        value = value >> bits;

        more = !value.is_zero();
        let data = if more { data | 0b10000000 } else { data };

        let data = if sign.into_neg_bit() && first {
            data | 0b01000000
        } else {
            data
        };

        if buf_len >= buf.len() {
            writer.write_all(&buf[..buf_len])?;
            buf_len = 0;
            written += buf_len;
        }

        buf[buf_len] = data;
        buf_len += 1;

        first = false;
    }

    if buf_len > 0 {
        writer.write_all(&buf[..buf_len])?;
        written += buf_len;
    }

    Ok(written)
}

/// Advised to use BufReader
pub fn read_unsigned_varint<I: UnsignedInt, R: io::Read>(
    mut reader: R,
) -> Result<I, VarIntReadError> {
    let mut value = I::ZERO;

    let mut byte = 0u8;
    let mut shift = 0;
    loop {
        reader.read_exact(slice::from_mut(&mut byte))?;

        let more = (byte & 0b10000000) != 0;
        let data = byte & 0b01111111;

        let shifted_data = I::from_u8_bits(data)
            .checked_shl(shift)
            .ok_or(VarIntReadError::ValueTooBig)?;

        value = value | shifted_data;

        if !more {
            break;
        }

        shift += 7;
    }

    Ok(value)
}

pub fn read_signed_varint<I: SignedInt, R: io::Read>(reader: R) -> Result<I, VarIntReadError> {
    let (value, sign) = read_varint_with_sign(reader)?;
    I::from_split_sign(value, sign).ok_or(VarIntReadError::InvalidSignedValue)
}

pub fn read_varint_with_sign<I: UnsignedInt, R: io::Read>(
    mut reader: R,
) -> Result<(I, Sign), VarIntReadError> {
    let mut value = I::ZERO;

    let mut byte = 0u8;
    let mut shift = 0;
    let mut sign = false;
    let mut first = true;

    loop {
        reader.read_exact(slice::from_mut(&mut byte))?;

        let (bits, mask) = if first {
            (6, 0b00111111)
        } else {
            (7, 0b01111111)
        };

        if first {
            sign = (byte & 0b01000000) != 0
        }

        first = false;

        let more = (byte & 0b10000000) != 0;
        let data = byte & mask;

        let shifted_data = I::from_u8_bits(data)
            .checked_shl(shift)
            .ok_or(VarIntReadError::ValueTooBig)?;

        value = value | shifted_data;

        if !more {
            break;
        }

        shift += bits;
    }

    Ok((value, Sign::from_neg_bit(sign)))
}

pub fn read_unsigned_varint_bits_le<R: io::Read>(
    mut reader: R,
    buf: &mut Vec<u8>,
) -> io::Result<usize> {
    let mut sr = 0u16;
    let mut sr_bits = 0u32;

    let mut byte = 0u8;
    let mut wrote = 0;

    let mut pending_zeros = 0usize;

    loop {
        reader.read_exact(slice::from_mut(&mut byte))?;

        let more = (byte & 0b10000000) != 0;
        let data = byte & 0b01111111;

        sr |= (data as u16) << sr_bits;
        sr_bits += 7;

        while sr_bits >= 8 || (!more && sr_bits > 0) {
            let byte = (sr & 0xff) as u8;
            if byte == 0 {
                pending_zeros += 1;
            } else {
                for _ in 0..pending_zeros {
                    buf.push(0);
                }
                buf.push(byte);
                wrote += pending_zeros + 1;
                pending_zeros = 0;
            }
            sr_bits = sr_bits.saturating_sub(8);
            sr >>= 8;
        }

        if !more {
            break;
        }
    }

    Ok(wrote)
}

pub fn copy_varint<S: io::Read + ?Sized, D: io::Write + ?Sized>(src: &mut S, dst: &mut D) -> Result<(), io::Error> {
    let mut buf = [0u8; 16];
    let mut buf_len = 0;

    loop {
        let mut byte = 0u8;
        src.read_exact(std::slice::from_mut(&mut byte))?;

        if buf_len >= buf.len() {
            dst.write_all(&buf[..buf_len])?;
            buf_len = 0;
        }

        buf[buf_len] = byte;
        buf_len += 1;

        if byte & 0x80 == 0 {
            break;
        }
    }

    if buf_len > 0 {
        dst.write_all(&buf[..buf_len])?;
    }

    Ok(())
}

pub fn is_varint_better(abs_leading_zeros: u32, bytewidth: u32, signed: bool) -> bool {
    let value_width = bytewidth * 8 - abs_leading_zeros;

    let rem_value_width = if signed {
        value_width.saturating_sub(6)
    } else {
        value_width.saturating_sub(7)
    };

    let extra_varint_bytes = rem_value_width.div_ceil(7);

    bytewidth > (extra_varint_bytes + 1)
}

macro_rules! impl_varint_primitives {
    ($($signed:ident:$unsigned:ident),*) => {

        $(

            impl UnsignedInt for $unsigned {
                const ZERO: Self = 0;

                fn is_zero(self) -> bool {
                    self == 0
                }

                fn into_u8_bits_trimmed(self) -> u8 {
                    (self & 0xff) as u8
                }

                fn from_u8_bits(val: u8) -> Self {
                    val as Self
                }

                fn checked_shl(self, shift: u32) -> Option<Self> {
                    $unsigned::checked_shl(self, shift)
                }
            }

            impl SignedInt for $signed {
                type Unsigned = $unsigned;

                fn into_split_sign(self) -> (Self::Unsigned, Sign) {
                    if self >= 0 {
                        (self as $unsigned, Sign::Positive)
                    } else {
                        (self.unsigned_abs(), Sign::Negative)
                    }
                }

                fn from_split_sign(val: Self::Unsigned, sign: Sign) -> Option<Self> {
                    if sign.is_positive() {
                        val.try_into().ok()
                    }
                    else if val == 0 || val > $signed::MIN.unsigned_abs() {
                        None
                    }
                    else {
                        let mid = (val - 1) as $signed;
                        Some((-mid) - 1)
                    }
                }
            }
        )*
    };
}

impl_varint_primitives!(i8:u8, i16:u16, i32:u32, i64:u64, i128:u128, isize:usize);

#[allow(clippy::unusual_byte_groupings)]
#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[test]
    fn test_unsigned_varint() {
        let mut vec = vec![];
        let value: u64 = 0b0010100_1001011_0101001_0100010_1001001;

        write_unsigned_varint(&mut vec, value).unwrap();

        assert_eq!(
            vec,
            [0b11001001, 0b10100010, 0b10101001, 0b11001011, 0b00010100]
        );

        let cur = io::Cursor::new(&vec);

        let read_value = read_unsigned_varint::<u64, _>(cur).unwrap();
        assert_eq!(read_value, value, "{read_value:x} != {value:x}");
    }

    #[test]
    fn test_varint_with_sign() {
        let mut vec = vec![];
        let value: u64 = 0b0101001_0100010_100100;

        write_varint_with_sign(&mut vec, value, Sign::Negative).unwrap();

        assert_eq!(vec, [0b11100100, 0b10100010, 0b00101001]);

        let cur = io::Cursor::new(&vec);

        let (read_value, read_sign) = read_varint_with_sign::<u64, _>(cur).unwrap();
        assert_eq!(
            (read_value, read_sign),
            (value, Sign::Negative),
            "{read_value:x} != {value:x}"
        );

        vec.clear();

        let value: u64 = 0b0100101_0010100_101010;

        write_varint_with_sign(&mut vec, value, Sign::Positive).unwrap();

        assert_eq!(vec, [0b10101010, 0b10010100, 0b00100101]);

        let cur = io::Cursor::new(&vec);

        let (read_value, read_sign) = read_varint_with_sign::<u64, _>(cur).unwrap();
        assert_eq!(
            (read_value, read_sign),
            (value, Sign::Positive),
            "{read_value:x} != {value:x}"
        );
    }

    #[test]
    fn test_signed_varint() {
        fn test_signed_varint_case(val: i64) {
            let mut vec = vec![];

            write_signed_varint(&mut vec, val).unwrap();

            let mut cur = io::Cursor::new(&vec);

            let read = read_signed_varint(&mut cur).unwrap();

            assert_eq!(val, read);
        }

        test_signed_varint_case(0);
        test_signed_varint_case(1);
        test_signed_varint_case(-1);
        test_signed_varint_case(76378764854327610);
        test_signed_varint_case(-7652837468765784187);
        test_signed_varint_case(i64::MIN);
        test_signed_varint_case(i64::MAX);
    }

    #[test]
    fn test_errors() {
        let invalid = [0xff; 16];

        let cur = io::Cursor::new(&invalid);

        let res = read_unsigned_varint::<u64, _>(cur);

        assert!(matches!(res, Err(VarIntReadError::ValueTooBig)));

        let neg_zero = [0b01000000];
        let cur = io::Cursor::new(&neg_zero);

        let res = read_signed_varint::<i64, _>(cur);

        assert!(matches!(res, Err(VarIntReadError::InvalidSignedValue)));
    }

    #[test]
    fn test_bits() {
        let value: u64 = 7687324876823828958;

        let mut vec = vec![];

        write_unsigned_varint(&mut vec, value).unwrap();

        let cur = io::Cursor::new(&vec);

        let mut bits = vec![];

        read_unsigned_varint_bits_le(cur, &mut bits).unwrap();

        assert!(bits.len() <= 8);

        while bits.len() < 8 {
            bits.push(0);
        }

        let bytes: [u8; 8] = bits.try_into().unwrap();

        let bit_value = u64::from_le_bytes(bytes);

        assert_eq!(bit_value, value, "{bit_value:x} != {value:x}");
    }

    #[test]
    fn test_is_varint_better() {
        let varint_short_values = [0x0u16, 0x7f, 0x0f];
        let varint_long_values = [0x80u16, 0xff];

        for v in varint_short_values {
            assert!(is_varint_better(v.leading_zeros(), 2, false));
        }

        for v in varint_long_values {
            assert!(!is_varint_better(v.leading_zeros(), 2, false));
        }

        let varint_shorter_values = [0x0u16, 0x3f, 0x0f];
        for v in varint_shorter_values {
            assert!(is_varint_better(v.leading_zeros(), 2, true));
        }

        assert!(!is_varint_better(0x7fu16.leading_zeros(), 2, true));

        let varint_short_values = [0x0u32, 0xffff, 0x0f];
        let varint_long_values = [0b1_0000000_0000000_0000000u32, 0xffffffff];

        for v in varint_short_values {
            assert!(is_varint_better(v.leading_zeros(), 4, false));
        }

        for v in varint_long_values {
            assert!(!is_varint_better(v.leading_zeros(), 4, false));
        }
    }
}
