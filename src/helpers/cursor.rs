use byteorder::{ByteOrder, ReadBytesExt};

pub trait ReadBytesExtensions: ReadBytesExt {
    fn read_compressed_u32<T: ByteOrder>(&mut self) -> Result<u32, std::io::Error>;
    fn read_compressed_i32<T: ByteOrder>(&mut self) -> Result<i32, std::io::Error>;
}

impl<R: ReadBytesExt> ReadBytesExtensions for R {
    // stolen from libil2cpp/utils/MemoryRead.cpp
    // thanks Stack
    fn read_compressed_u32<T: ByteOrder>(&mut self) -> Result<u32, std::io::Error> {
        let mut val: u32 = 0;
        let read = self.read_u8()?;

        if (read & 0x80) == 0 {
            // 1 byte written
            val = read as u32;
        } else if (read & 0xC0) == 0x80 {
            // 2 bytes written
            val = (read as u32 & !0x80) << 8;
            val |= self.read_u8()? as u32;
        } else if (read & 0xE0) == 0xC0 {
            // 4 bytes written
            val = (read as u32 & !0xC0) << 24;
            val |= (self.read_u8()? as u32) << 16;
            val |= (self.read_u8()? as u32) << 8;
            val |= self.read_u8()? as u32;
        } else if read == 0xF0 {
            // 5 bytes written, we had a really large int32!
            val = self.read_u32::<T>()?;
        } else if read == 0xFE {
            // Special encoding for Int32.MaxValue
            val = u32::MAX - 1;
        } else if read == 0xFF {
            // Yes we treat UInt32.MaxValue (and Int32.MinValue, see ReadCompressedInt32) specially
            val = u32::MAX;
        } else {
            panic!("Invalid compressed integer format");
        }

        Ok(val)
    }

    fn read_compressed_i32<T: ByteOrder>(&mut self) -> Result<i32, std::io::Error> {
        let mut encoded = self.read_compressed_u32::<T>()?;

        // -UINT32_MAX can't be represted safely in an int32_t, so we treat it specially
        if encoded == u32::MAX {
            return Ok(i32::MIN);
        }

        let is_negative: bool = (encoded & 1) != 0;
        encoded >>= 1;
        let result = if is_negative {
            -((encoded + 1) as i32)
        } else {
            encoded as i32
        };

        Ok(result)
    }
}
