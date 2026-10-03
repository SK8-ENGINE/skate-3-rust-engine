//! Embedded XDVDFS (XISO) extractor. Replaces the external extract-xiso
//! binary with the CLI subset the asset pipeline uses: `-x <image> -d <dir>`.
//! OffsetWrapper probes RAW/XGD1-3 layouts, so redump-style Xbox 360 images
//! are accepted the same way extract-xiso accepts them.
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};
use xdvdfs::blockdev::{BlockDeviceRead, OffsetWrapper};

fn run() -> Result<(), String> {
    let mut image = None;
    let mut dest = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-x") => image = Some(PathBuf::from(args.next().ok_or("missing image after -x")?)),
            Some("-d") => dest = Some(PathBuf::from(args.next().ok_or("missing directory after -d")?)),
            _ => return Err(format!("unsupported argument: {}", arg.to_string_lossy())),
        }
    }
    let (image, dest) = match (image, dest) {
        (Some(image), Some(dest)) => (image, dest),
        _ => return Err("usage: skate-xiso -x <image.iso> -d <output-directory>".into()),
    };
    let file = fs::File::open(&image).map_err(|e| format!("could not open image: {e}"))?;
    let mut dev = OffsetWrapper::new(io::BufReader::new(file))
        .map_err(|_| "not an XDVDFS/XISO image".to_string())?;
    let volume =
        xdvdfs::read::read_volume(&mut dev).map_err(|_| "invalid XDVDFS volume".to_string())?;
    fs::create_dir_all(&dest).map_err(|e| format!("could not create output directory: {e}"))?;
    let tree = volume
        .root_table
        .file_tree(&mut dev)
        .map_err(|e| format!("could not walk image: {e:?}"))?;
    for (parent, node) in tree {
        let name = node
            .name_str()
            .map_err(|_: xdvdfs::util::Error<io::Error>| "unsupported file name encoding".to_string())?;
        let directory = checked_dir(&dest, &parent)?;
        fs::create_dir_all(&directory).map_err(|e| format!("could not create {parent}: {e}"))?;
        let path = directory.join(checked_name(&name)?);
        // Dirent data lives in a packed on-disk struct; copy it out before use.
        let dirent = node.node.dirent;
        if dirent.is_directory() {
            fs::create_dir_all(&path).map_err(|e| format!("could not create {parent}/{name}: {e}"))?;
            continue;
        }
        println!("{parent}/{name}");
        extract_file(&mut dev, dirent, &path)
            .map_err(|e| format!("could not extract {parent}/{name}: {e}"))?;
    }
    Ok(())
}

/// Image paths and names must stay inside the extraction tree.
fn checked_name(name: &str) -> Result<&str, String> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
        return Err(format!("unsafe file name in image: {name:?}"));
    }
    Ok(name)
}

fn checked_dir(dest: &Path, parent: &str) -> Result<PathBuf, String> {
    let mut directory = dest.to_path_buf();
    for component in parent.split('/').filter(|c| !c.is_empty()) {
        directory.push(checked_name(component)?);
    }
    Ok(directory)
}

fn extract_file(
    dev: &mut impl BlockDeviceRead<io::Error>,
    dirent: xdvdfs::layout::DirectoryEntryDiskData,
    path: &Path,
) -> Result<(), String> {
    let mut output =
        io::BufWriter::new(fs::File::create(path).map_err(|e| e.to_string())?);
    let region = dirent.data;
    let mut buffer = vec![0; 4 * 1024 * 1024];
    let mut offset = 0u32;
    while offset < region.size {
        let chunk = (region.size - offset).min(buffer.len() as u32) as usize;
        let absolute = region
            .offset(offset)
            .map_err(|e: xdvdfs::util::Error<io::Error>| format!("{e:?}"))?;
        dev.read(absolute, &mut buffer[..chunk])
            .map_err(|e| e.to_string())?;
        output.write_all(&buffer[..chunk]).map_err(|e| e.to_string())?;
        offset += chunk as u32;
    }
    output.flush().map_err(|e| e.to_string())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("skate-xiso: {error}");
        std::process::exit(1);
    }
}
