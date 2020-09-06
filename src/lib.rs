#![feature(new_uninit)]

use std::path::{PathBuf, Path};

mod file;
use file::FileToZip;
use std::io::{Write, BufWriter};

pub struct ZipStream<W: Write> {
    sink: BufWriter<W>,
    files: Vec<FileToZip>,
    
    // 128kb based on https://eklitzke.org/efficient-file-copying-on-linux
    #[cfg(not(target_os = "windows"))]
    buffer: Box<[u8; 131072]>,
    #[cfg(target_os = "windows")]
    buffer: Box<[u8; 1024 * 512]>,
    bytes_written: u64
}

impl<W> ZipStream<W> where W: Write {

    pub fn add_file(&mut self, path: PathBuf, zip_path: String) -> std::io::Result<u64> {
        let mut ftz = FileToZip::create(path, zip_path)?;
        let added = ftz.write_file_entry(&mut self.sink, self.bytes_written, self.buffer.as_mut())?;
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

        // remember start of zip64 eocd for locator
        let zip64_end_start = self.bytes_written;
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
        // central_dir_bytes += 40; those dont count for central dir size

        // size of central dir
        self.sink.write_all(&central_dir_bytes.to_le_bytes())?;

        // offset of central dir
        self.sink.write_all(&start_of_central_dir.to_le_bytes())?;

        self.bytes_written += 56;

        // 3. zip64 eocd locator
        self.sink.write_all(&0x07064b50u32.to_le_bytes())?;
        self.sink.write_all(&0u32.to_le_bytes())?;

        // 8 byte relative offset of zip64 eocd record
        self.sink.write_all(&zip64_end_start.to_le_bytes())?;

        // number of disks
        self.sink.write_all(&1u32.to_le_bytes())?;
        self.bytes_written += 20;

        // 4. eocd record
        self.sink.write_all(&0x06054b50u32.to_le_bytes())?;

        // #this disk + # disk eocd start
        self.sink.write_all(&0u32.to_le_bytes())?;

        // total number of entries (set to 0xffff to use zip64) + on this disk
        self.sink.write_all(&u32::MAX.to_le_bytes())?;

        // size of central dir 4 bytes (use zip64 one) + offset 4 bytes (use zip64)
        self.sink.write_all(&u64::MAX.to_le_bytes())?;

        // zip comment length
        self.sink.write_all(&[0, 0])?;

        self.sink.flush()?;
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
        let path_base = folder.to_str().unwrap();

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
                            stream.add_file(path, zip_path)?;
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

impl<W> From<W> for ZipStream<W> where W: Write {
    fn from(w: W) -> Self {
        Self {
            // for headers and dirs
            sink: BufWriter::with_capacity(4096, w),
            files: Vec::new(),
            buffer: unsafe { Box::new_zeroed().assume_init() },
            bytes_written: 0
        }
    }
}

#[cfg(test)]
mod test {

    #[test]
    fn from_eq_to() {}
}
