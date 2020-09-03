use std::path::PathBuf;
struct FileToZip {
    last_mod_time: u16,
    last_mod_date: u16,
    crc32: u32,
    size: u64,
    file_name: String,
    original_path: PathBuf,
    offset_of_fh: u64,
    use_data_descr: bool,
}

impl FileToZip {
    /// Creates a new FileToZip, checks if file exists and if the crc should be
    /// calculated after writing the data (filesize > 4KiB)
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
            crc32: if size > 4096 { 0 } else { unimplemented!() },
            file_name: zip_path,
            original_path: path,
            offset_of_fh: 0,
            use_data_descr: size > 4096
        })
    }

    fn write_file_entry<W: std::io::Write>(&mut self, mut writer: W, offset_to_start: u64) -> std::io::Result<()> {
        // local file header
        // magic number
        self.offset_of_fh = offset_to_start;
        writer.write_all(&0x04034b50u32.to_le_bytes())?;
        // version needed: at least 4.5 (zip64 ext)
        writer.write_all(&45u16.to_le_bytes())?;
        // general purpose bitflags
        let flags: u16 = 
            // bit 3: write crc32, compressed size and uncompressed size in data descriptor
            0b1000 |
            // bit 11: names are utf8
            0b1000_0000_0000
        ;
        writer.write_all(&flags.to_le_bytes())?;
        // compression method: stored
        writer.write_all(&0u16.to_le_bytes())?;
        
        // mod time
        writer.write_all(&self.last_mod_time.to_le_bytes())?;
        // mod date
        writer.write_all(&self.last_mod_date.to_le_bytes())?;  
        // crc, sizes (3x4byte)
        writer.write_all(&[0, 0, 0, 0,  0, 0, 0, 0,  0, 0, 0, 0])?;

        // file name length
        writer.write_all(&(self.file_name.len() as u16).to_le_bytes())?;

        // extra field length
        todo!();

        // write file content
        let mut file = std::fs::File::open(self.original_path)?;
        let copy_size = std::io::copy(&mut file, &mut writer)?;
        assert_eq!(copy_size as u64, self.size);

        // write data descriptor signature
        writer.write_all(&0x08074b50u32.to_le_bytes())?;

        todo!("data descriptor")
    }

    fn write_central_dir_entry<W: Write>(&self, writer: W) {

    }
}

pub struct ZipStream<W> {
    sink: W,
    files: Vec<FileToZip>,
}

#[cfg(test)]
mod test {

    #[test]
    fn from_eq_to() {}
}
