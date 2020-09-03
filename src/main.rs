fn main() {
    println!("Zip large files and folders");
    let mut args: Vec<_> = std::env::args().collect();
    if args.len() < 3 {
        println!("Usage: zipstream [<file> / <folder>] <targetfile.zip>");
        return;
    }

    let target_name = &args[args.len()-1];
    println!("Writing files to {}", target_name);
    let target_path = std::path::Path::new(target_name);
    let mut file = match std::fs::File::create(target_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Failed to create zip file: {:?}", e);
            return;
        }
    };
}
