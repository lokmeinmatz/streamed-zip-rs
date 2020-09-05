#![feature(new_uninit)]

use std::path::{PathBuf, Path};
use crc32fast::Hasher;
use std::io::Read;


struct FileToZip {
    last_mod_time: u16,
    last_mod_date: u16,
    crc32: Option<u32>,
    size: u64,
    file_name: String,
    original_path: PathBuf,
    offset_of_fh: u64,
}

impl FileToZip {
    /// Creates a new FileToZip, checks if file exists
    fn create(path: PathBuf, zip_path: String) -> std::io::Result<Self> {
        if !path.exists() || !path.is_file() {
            return Err(std::io::Error::from(std::io::ErrorKind::NotFound));
        }
        let meta = path.metadata().unwrap();
        let unix_t: std::time::Duration = meta
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap();
        use chrono::{Datelike, Timelike, TimeZone};
        let datetime = chrono::Utc.timestamp(unix_t.as_secs() as i64, 0);
        let msdos_time = ((datetime.second() as u16) >> 1) | ((datetime.minute() as u16) << 5) | ((datetime.hour() as u16) << 11);
        let msdos_date = (datetime.day() as u16) | ((datetime.month() as u16) << 5) | ((datetime.year() as u16 - 1980) << 9);
        let size = meta.len();

        Ok(Self {
            last_mod_date: msdos_date,
            last_mod_time: msdos_time,
            size,
            crc32: None,
            file_name: zip_path,
            original_path: path,
            offset_of_fh: 0
        })
    }

    /// writes file entry including file itself, returns bytes written.
    fn write_file_entry<W: std::io::Write>(&mut self, mut writer: W, offset_to_start: u64) -> std::io::Result<u64> {
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
        // b40 + name_len

        // write file content
        let mut file = std::fs::File::open(&self.original_path)?;

        // copy 4Kib per transfer
        let mut crc32 = Hasher::new();
        let mut bytes_written: u64 = 0;
        let mut buffer: Box<[u8; 4096]> = unsafe { Box::new_zeroed().assume_init() };
        loop {
            let read_bytes = file.read(&mut *buffer)?;
            if read_bytes == 0 {
                break;
            }
            crc32.update(&*buffer);
            writer.write_all(&buffer[0..read_bytes])?;
            bytes_written += read_bytes as u64;
        }
        // b40 + name_len + file_len

        assert_eq!(bytes_written as u64, self.size);

        // write data descriptor signature
        writer.write_all(&0x08074b50u32.to_le_bytes())?;
        // b44 + name_len + file_len
        
        // write 4 byte crc32
        let crc = crc32.finalize();
        writer.write_all(&crc.to_le_bytes())?;
        self.crc32 = Some(crc);
        // b48 + name_len + file_len
        
        // write 8bytes uncompressed + 8 bytes compressed size
        writer.write_all(&bytes_written.to_le_bytes())?;
        writer.write_all(&bytes_written.to_le_bytes())?;
        // b64 + name_len + file_len
        Ok(64u64 + (self.file_name.len() as u64) + bytes_written)
    }

    fn write_central_dir_entry<W: std::io::Write>(&self, mut writer: W) -> std::io::Result<u64> {
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
        writer.write_all(&0xffff_ffff_ffff_ffff_u64.to_le_bytes())?;
        // b28

        // file name length
        writer.write_all(&(self.file_name.len() as u16).to_le_bytes())?;
        // b30
        // extra field length
        // 20 bytes
        //   2 extra type
        //   2 extra size
        //   2x8 sizes (equal)
        writer.write_all(&20_u16.to_le_bytes())?;
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
        // header id: 1, size: 28 (le bytes)
        writer.write_all(&[1, 0, 28, 0])?;
        // b50 + name_len
        
        // uncompressed + compressed size
        writer.write_all(&self.size.to_le_bytes())?;
        writer.write_all(&self.size.to_le_bytes())?;
        // b66 + name_len
        
        // offset
        writer.write_all(&self.offset_of_fh.to_le_bytes())?;
        // b74 + name_len
        
    
        Ok(64 + self.file_name.len() as u64)
    }
}

pub struct ZipStream<W> {
    sink: W,
    files: Vec<FileToZip>,
    bytes_written: u64
}

impl<W> ZipStream<W> where W: std::io::Write {

    pub fn add_file(&mut self, path: PathBuf, zip_path: String) -> std::io::Result<u64> {
        let mut ftz = FileToZip::create(path, zip_path)?;
        let added = ftz.write_file_entry(&mut self.sink, self.bytes_written)?;
        self.files.push(ftz);
        self.bytes_written += added;
        Ok(added)
    }

    pub fn finalize(&mut self) -> std::io::Result<u64> {

        // 1. write file central directory headers
        // 2. zip64 eocd (end of central directory) record
        // 3. zip64 eocd locator
        // 4. eocd record
        let start_of_central_dir = self.bytes_written;
        let mut central_dir_bytes = 0u64;
        // 1.
        for file in &self.files {
            let a = file.write_central_dir_entry(&mut self.sink)?;
            self.bytes_written += a;
            central_dir_bytes += a;
        }

        // 2.

        // zip64 eocd signature
        self.sink.write_all(&0x06064b50u32.to_le_bytes())?;
        
        // size of zip64 eocd record
        self.sink.write_all(&44u64.to_le_bytes())?;

        // version made by
        self.sink.write_all(&[45, 3])?;
    
        // version needed: at least 4.5 (zip64 ext)
        self.sink.write_all(&45u16.to_le_bytes())?;

        // number this disk + number start disk
        self.sink.write_all(&0u64.to_le_bytes())?;
        // b24

        // entries cd this disk + all disks
        self.sink.write_all(&(self.files.len() as u64).to_le_bytes())?;
        self.sink.write_all(&(self.files.len() as u64).to_le_bytes())?;
        // b40
        central_dir_bytes += 40;

        // size of central dir
        self.sink.write_all(&central_dir_bytes.to_le_bytes())?;

        // offset of central dir
        self.sink.write_all(&start_of_central_dir.to_le_bytes())?;

        self.bytes_written += 56;
        Ok(self.bytes_written)
    }

    pub fn stream_folder(writer: W, folder: &Path) -> std::io::Result<u64> {
        if !folder.is_dir() {
            return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput));
        }
        let mut stream = ZipStream::from(writer);
        // add all files recursivly to zipstream
        let mut path_stack = Vec::new();
        path_stack.push(folder.to_owned());
        let path_base = dbg!(folder.to_str().unwrap());

        // add all child files
        while let Some(curr) = path_stack.pop() {
            for e in std::fs::read_dir(curr)? {
                match e {
                    Ok(dir_entry) => {
                        let f_type = dir_entry.file_type()?;
                        if f_type.is_dir() {
                            path_stack.push(dir_entry.path());
                        } else {
                            let path = dir_entry.path();
                            let mut zip_path = path.to_str().unwrap().to_owned();
                            zip_path.replace_range(0..path_base.len(), "");
                            stream.add_file(path, dbg!(zip_path))?;
                        }
                    },
                    Err(err) => eprintln!("{:?}", err)
                }
            }
        }

        // writes central directory entry
        stream.finalize()
    }
}

impl<W> From<W> for ZipStream<W> where W: std::io::Write {
    fn from(w: W) -> Self {
        Self {
            sink: w,
            files: Vec::new(),
            bytes_written: 0
        }
    }
}

#[cfg(test)]
mod test {

    #[test]
    fn from_eq_to() {}
}
