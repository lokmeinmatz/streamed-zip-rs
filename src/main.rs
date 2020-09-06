use streamed_zip_rs::ZipStream;
use std::time::Instant;
fn main() {



    println!("Zip large files and folders");
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        println!("Usage: zipstream <folder> <targetfile.zip>");
        return;
    }
    let start = Instant::now();
    let src_name = &args[0];
    let target_name = &args[1];
    println!("Writing {} to {}", src_name, target_name);
    let target_path = std::path::Path::new(target_name);
    let file = match std::fs::File::create(target_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Failed to create zip file: {:?}", e);
            return;
        }
    };

    let bytes = ZipStream::stream_folder(file, std::path::Path::new(src_name)).unwrap();
    println!("streamed folder");
    let elapsed = start.elapsed();
    println!("Took {}s to stream {} bytes", elapsed.as_secs_f32(), bytes);
}
