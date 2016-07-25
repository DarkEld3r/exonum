use std::mem;
use std::net::{SocketAddr, SocketAddrV4, Ipv4Addr};

use time::{Timespec};
use byteorder::{ByteOrder, LittleEndian};

use super::super::crypto::{Hash, PublicKey};

use super::Error;

pub trait Field<'a> {
    // TODO: use Read and Cursor
    // TODO: debug_assert_eq!(to-from == size of Self)
    fn read(buffer: &'a [u8], from: usize, to: usize) -> Self;
    fn write(&self, buffer: &'a mut Vec<u8>, from: usize, to: usize);

    #[allow(unused_variables)]
    fn check(buffer: &'a [u8], from: usize, to: usize)
        -> Result<(), Error> {
        Ok(())
    }
}

impl<'a> Field<'a> for bool {
    fn read(buffer: &'a [u8], from: usize, _: usize) -> bool {
        buffer[from] == 1
    }

    fn write(&self, buffer: &'a mut Vec<u8>, from: usize, _: usize) {
        buffer[from] = if *self {1} else {0}
    }

    fn check(buffer: &'a [u8], from: usize, _: usize)
        -> Result<(), Error> {
        if buffer[from] != 0 && buffer[from] != 1 {
            Err(Error::IncorrectBoolean {
                position: from as u32,
                value: buffer[from]
            })
        } else {
            Ok(())
        }
    }
}

impl<'a> Field<'a> for u32 {
    fn read(buffer: &'a [u8], from: usize, to: usize) -> u32 {
        LittleEndian::read_u32(&buffer[from..to])
    }

    fn write(&self, buffer: &'a mut Vec<u8>, from: usize, to: usize) {
        LittleEndian::write_u32(&mut buffer[from..to], *self)
    }
}

impl<'a> Field<'a> for u64 {
    fn read(buffer: &'a [u8], from: usize, to: usize) -> u64 {
        LittleEndian::read_u64(&buffer[from..to])
    }

    fn write(&self, buffer: &'a mut Vec<u8>, from: usize, to: usize) {
        LittleEndian::write_u64(&mut buffer[from..to], *self)
    }
}

impl<'a> Field<'a> for &'a Hash {
    fn read(buffer: &'a [u8], from: usize, _: usize) -> &'a Hash {
        unsafe {
            mem::transmute(&buffer[from])
        }
    }

    fn write(&self, buffer: &'a mut Vec<u8>, from: usize, to: usize) {
        &mut buffer[from..to].copy_from_slice(self.as_ref());
    }
}

impl<'a> Field<'a> for &'a PublicKey {
    fn read(buffer: &'a [u8], from: usize, _: usize) -> &'a PublicKey {
        unsafe {
            mem::transmute(&buffer[from])
        }
    }

    fn write(&self, buffer: &'a mut Vec<u8>, from: usize, to: usize) {
        &mut buffer[from..to].copy_from_slice(self.as_ref());
    }
}

impl<'a> Field<'a> for Timespec {
    fn read(buffer: &'a [u8], from: usize, to: usize) -> Timespec {
        let nsec = LittleEndian::read_u64(&buffer[from..to]);
        Timespec {
            sec:  (nsec / 1_000_000_000) as i64,
            nsec: (nsec % 1_000_000_000) as i32,
        }
    }

    fn write(&self, buffer: &'a mut Vec<u8>, from: usize, to: usize) {
        let nsec = (self.sec as u64) * 1_000_000_000 + self.nsec as u64;
        LittleEndian::write_u64(&mut buffer[from..to], nsec)
    }
}

impl<'a> Field<'a> for SocketAddr {
    fn read(buffer: &'a [u8], from: usize, to: usize) -> SocketAddr {
        let ip = Ipv4Addr::new(buffer[from+0], buffer[from+1],
                               buffer[from+2], buffer[from+3]);
        let port = LittleEndian::read_u16(&buffer[from+4..to]);
        SocketAddr::V4(SocketAddrV4::new(ip, port))
    }

    fn write(&self, buffer: &'a mut Vec<u8>, from: usize, to: usize) {
        match *self {
            SocketAddr::V4(addr) => {
                &mut buffer[from..to-2].copy_from_slice(&addr.ip().octets());
            },
            SocketAddr::V6(_) => {
                // FIXME: Supporting Ipv6
                panic!("Ipv6 are currently unsupported")
            },
        }
        LittleEndian::write_u16(&mut buffer[to-2..to], self.port());
    }
}

pub trait SegmentField<'a> {
    const ITEM_SIZE: usize;

    fn from_slice(slice: &'a [u8]) -> Self;
    fn as_slice(&self) -> &'a [u8];
    fn count(&self) -> u32;

    #[allow(unused_variables)]
    fn check_data(slice: &'a [u8], pos: u32) -> Result<(), Error> {
        Ok(())
    }
}

impl<'a, T> Field<'a> for T where T: SegmentField<'a> {
    fn read(buffer: &'a [u8], from: usize, to: usize) -> T {
        unsafe {
            let pos = LittleEndian::read_u32(&buffer[from..from+4]);
            let len = LittleEndian::read_u32(&buffer[from+4..to]);
            let ptr = buffer.as_ptr().offset(pos as isize);
            Self::from_slice(
                ::std::slice::from_raw_parts(ptr as *const u8, len as usize)
            )
        }
    }

    fn write(&self, buffer: &'a mut Vec<u8>, from: usize, to: usize) {
        let pos = buffer.len();
        LittleEndian::write_u32(&mut buffer[from..from+4], pos as u32);
        LittleEndian::write_u32(&mut buffer[from+4..to], self.count());
        buffer.extend_from_slice(self.as_slice());
    }

    fn check(buffer: &'a [u8], from: usize, to: usize)
        -> Result<(), Error> {
        let pos = LittleEndian::read_u32(&buffer[from..from+4]);
        let count = LittleEndian::read_u32(&buffer[from+4..to]);

        if count == 0 {
            return Ok(())
        }

        let start = pos as usize;

        if start < from + 8 {
            return Err(Error::IncorrectSegmentRefference {
                position: from as u32,
                value: pos
            })
        }

        let end = start + Self::ITEM_SIZE * (count as usize);

        if end > buffer.len() {
            return Err(Error::IncorrectSegmentSize {
                position: (from + 4) as u32,
                value: count
            })
        }

        return Self::check_data(unsafe {
            ::std::slice::from_raw_parts(pos as *const u8, count as usize)
        }, from as u32);
    }
}

impl<'a> SegmentField<'a> for &'a [u8] {
    const ITEM_SIZE: usize = 1;

    fn from_slice(slice: &'a [u8]) -> Self {
        slice
    }

    fn as_slice(&self) -> &'a [u8] {
        self
    }

    fn count(&self) -> u32 {
        self.len() as u32
    }
}

impl<'a> SegmentField<'a> for &'a [Hash] {
    const ITEM_SIZE: usize = 32;

    fn from_slice(slice: &'a [u8]) -> Self {
        unsafe {
            ::std::slice::from_raw_parts(slice.as_ptr() as *const Hash,
                                         slice.len() / 32)
        }
    }

    fn as_slice(&self) -> &'a [u8] {
        unsafe {
            ::std::slice::from_raw_parts(self.as_ptr() as *const u8,
                                         self.len() * 32)
        }
    }

    fn count(&self) -> u32 {
        self.len() as u32
    }
}

impl<'a> SegmentField<'a> for &'a str {
    const ITEM_SIZE: usize = 32;

    fn from_slice(slice: &'a [u8]) -> Self {
        unsafe {
            ::std::str::from_utf8_unchecked(slice)
        }
    }

    fn as_slice(&self) -> &'a [u8] {
        self.as_bytes()
    }

    fn count(&self) -> u32 {
        self.len() as u32
    }

    fn check_data(slice: &'a [u8], pos: u32) -> Result<(), Error> {
        if let Err(e) = ::std::str::from_utf8(slice) {
            return Err(Error::Utf8 {
                position: pos,
                error: e,
            });
        }
        Ok(())
    }
}
