use std::path::PathBuf;
use crc32fast::Hasher;
use std::io::{Read, Write, BufWriter};

pub struct FileToZip {
    last_mod_time: u16,
    last_mod_date: u16,
    crc32: Option<u32>,
    size: Option<u64>,
    file_name: String,
    source: Option<Box<dyn Read>>,
    offset_of_fh: u64,
}

use chrono::{Datelike, Timelike, TimeZone};

impl FileToZip {
    /// Creates a new FileToZip, checks if file exists
    pub fn from_file(path: PathBuf, mut zip_path: String) -> std::io::Result<Self> {
        

        let file = std::fs::File::open(&path)?;

        zip_path = zip_path.replace("\\", "/");
        if zip_path.starts_with("/") {
            zip_path.remove(0);
        }
        let meta = path.metadata().unwrap();
        let unix_t: std::time::Duration = meta
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap();
        
        let datetime = chrono::Utc.timestamp(unix_t.as_secs() as i64, 0);
        // black magic msdos time
        let msdos_time = ((datetime.second() as u16) >> 1) | ((datetime.minute() as u16) << 5) | ((datetime.hour() as u16) << 11);
        let msdos_date = (datetime.day() as u16) | ((datetime.month() as u16) << 5) | ((datetime.year() as u16 - 1980) << 9);
        
        Ok(Self {
            last_mod_date: msdos_date,
            last_mod_time: msdos_time,
            size: None,
            crc32: None,
            file_name: zip_path,
            source: Some(Box::new(file)),
            offset_of_fh: 0
        })
    }

    /// can use any Read-implementing source as data
    /// if `last_mod = None`, the current utc time `chrono::Utc::now()` is used.
    #[allow(dead_code)]
    pub fn from_reader(source: Box<dyn Read>, zip_path: String, last_mod: Option<chrono::DateTime<chrono::Utc>>) -> std::io::Result<Self> {
        let datetime = last_mod.unwrap_or(chrono::Utc::now());
        // black magic msdos time
        let msdos_time = ((datetime.second() as u16) >> 1) | ((datetime.minute() as u16) << 5) | ((datetime.hour() as u16) << 11);
        let msdos_date = (datetime.day() as u16) | ((datetime.month() as u16) << 5) | ((datetime.year() as u16 - 1980) << 9);

        Ok(Self {
            last_mod_date: msdos_date,
            last_mod_time: msdos_time,
            size: None,
            crc32: None,
            file_name: zip_path,
            source: Some(source),
            offset_of_fh: 0
        })
    }

    /// writes file entry including file itself, returns bytes written and the underlying boxed source
    pub(crate) fn write_file_entry<W: std::io::Write>(&mut self, writer: &mut BufWriter<W>, offset_to_start: u64, buffer: &mut [u8]) -> std::io::Result<(u64, Box<dyn Read>)> {
        // local file header
        // magic number
        self.offset_of_fh = offset_to_start;
        writer.write_all(&0x04034b50u32.to_le_bytes())?;
        // b4
        
        // version needed: at least 4.5 (zip64 ext)
        writer.write_all(&45u16.to_le_bytes())?;
        // b6
        
        // general purpose bitflags
        let flags: u16 = 
            // bit 3: write crc32, compressed size and uncompressed size in data descriptor
            0b1000 |
            // bit 11: names are utf8
            0b1000_0000_0000
        ;
        writer.write_all(&flags.to_le_bytes())?;
        // b8
        
        // compression method: stored
        writer.write_all(&0u16.to_le_bytes())?;
        //b10

        // mod time
        writer.write_all(&self.last_mod_time.to_le_bytes())?;
        // mod date
        writer.write_all(&self.last_mod_date.to_le_bytes())?;  
        // b14
        // crc
        writer.write_all(&0_u32.to_le_bytes())?;
        // b18

        // compressed / uncompressed size: use zip64 regardless of size (easier)
        writer.write_all(&0xffff_ffff_ffff_ffff_u64.to_le_bytes())?;
        // b26

        // file name length
        writer.write_all(&(self.file_name.len() as u16).to_le_bytes())?;
        // b28
        // extra field length
        // 20 bytes
        //   2 extra type
        //   2 extra size
        //   2x8 sizes (equal)
        writer.write_all(&20_u16.to_le_bytes())?;
        //b 30
        writer.write_all(self.file_name.as_bytes())?;
        // b30 + name_len

        // write extra zip64 header
        // header id: 1, size: 16 (le bytes)
        writer.write_all(&[1, 0, 16, 0])?;
        writer.write_all(&u128::MAX.to_le_bytes())?;
        // b50 + name_len

        // write file content
        let mut src = self.source.take().ok_or(std::io::Error::new(std::io::ErrorKind::InvalidData, "No source in ZipFile found"))?;

        // copy
        let mut crc32 = Hasher::new();
        let mut bytes_written: u64 = 0;
        writer.flush()?;
        loop {
            let read_bytes = src.read(buffer)?;
            if read_bytes == 0 {
                break;
            }
            crc32.update(&buffer[0..read_bytes]);
            writer.get_mut().write_all(&buffer[0..read_bytes])?;
            bytes_written += read_bytes as u64;
        }
        // b50 + name_len + file_len

        // store the bytes written for the central directory entry
        self.size = Some(bytes_written);

        // write data descriptor signature
        writer.write_all(&0x08074b50u32.to_le_bytes())?;
        // b54 + name_len + file_len
        
        // write 4 byte crc32
        let crc = crc32.finalize();
        writer.write_all(&crc.to_le_bytes())?;
        //println!("CRC of {:?}: {:x}", &self.original_path, crc);
        self.crc32 = Some(crc);
        // b58 + name_len + file_len
        
        // write 8bytes uncompressed + 8 bytes compressed size
        writer.write_all(&bytes_written.to_le_bytes())?;
        writer.write_all(&bytes_written.to_le_bytes())?;
        // b74 + name_len + file_len
        Ok((74u64 + (self.file_name.len() as u64) + bytes_written, src))
    }

    /// Writes the central directory entry.
    /// Only call after write_file_entry was executed, otherwise this must fail because the written size, crc etc. are unknown.
    pub(crate) fn write_central_dir_entry<W: std::io::Write>(&mut self, mut writer: W) -> std::io::Result<u64> {
        // central directory file header
        // magic number

        writer.write_all(&0x02014b50u32.to_le_bytes())?;
        // b4
        
        // version made by: ??? at least 4.5 (zip64 ext) | 3 = unix
        writer.write_all(&[45, 3])?;
        // b6
        // version needed: at least 4.5 (zip64 ext)
        writer.write_all(&45u16.to_le_bytes())?;
        // b8
        
        // general purpose bitflags
        let flags: u16 = 
            // bit 3: write crc32, compressed size and uncompressed size in data descriptor
            0b1000 |
            // bit 11: names are utf8
            0b1000_0000_0000
        ;
        writer.write_all(&flags.to_le_bytes())?;
        // b10
        
        // compression method: stored
        writer.write_all(&0u16.to_le_bytes())?;
        //b12

        // mod time
        writer.write_all(&self.last_mod_time.to_le_bytes())?;
        // mod date
        writer.write_all(&self.last_mod_date.to_le_bytes())?;  
        // b16
        // crc
        writer.write_all(&self.crc32.expect(
            "File must be first written before central dir entry is created!"
        ).to_le_bytes())?;
        // b20

        // compressed / uncompressed size: use zip64 regardless of size (easier)
        writer.write_all(&u64::MAX.to_le_bytes())?;
        // b28

        // file name length
        writer.write_all(&(self.file_name.len() as u16).to_le_bytes())?;
        // b30
        // extra field length
        // 28 bytes
        //   2 extra type
        //   2 extra size
        //   3x8 sizes (equal)
        writer.write_all(&28_u16.to_le_bytes())?;
        //b 32
        
        // file comment (2), disknumber (2), internal (2), external (4)
        writer.write_all(&[0u8; 10])?;
        // b42
        
        // relative offset local header: set to ffff for zip64
        writer.write_all(&u32::MAX.to_le_bytes())?;
        // b46

        writer.write_all(self.file_name.as_bytes())?;
        // b46 + name_len
        
        // write extra zip64 field  4.5.3 APPNOTE
        // header id: 1, size: 24 (without these 4 bytes) (le bytes)
        writer.write_all(&[1, 0, 24, 0])?;
        // b50 + name_len
        
        let size = self.size.take().ok_or(std::io::Error::new(std::io::ErrorKind::InvalidData, "Size was None: call write_file_entry first!"))?;
        // uncompressed + compressed size
        writer.write_all(&size.to_le_bytes())?;
        writer.write_all(&size.to_le_bytes())?;
        // b66 + name_len
        
        // offset
        writer.write_all(&self.offset_of_fh.to_le_bytes())?;
        // b74 + name_len
        
    
        Ok(74 + self.file_name.len() as u64)
    }
}