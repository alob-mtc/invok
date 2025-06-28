use std::fs::{self, File};
use std::io::{self, Cursor, Write};
use std::path::{Path, PathBuf};
use tar::{Builder, Header};
use zip::write::FileOptions;
use zip::{ZipArchive, ZipWriter};

pub fn to_camel_case_handler(input: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = false;

    for (i, c) in input.chars().enumerate() {
        if c == '-' {
            capitalize_next = true;
        } else if capitalize_next || i == 0 {
            result.push(c.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }

    result.push_str("Handler");
    result
}

/// Compresses the contents of a directory into a ZIP file, excluding specified files.
///
/// # Arguments
///
/// * `src_dir` - The source directory to compress.
/// * `dest_zip` - The path to the destination ZIP file.
/// * `excludes` - A list of file names to exclude from compression.
///
/// # Returns
///
/// Result<(), io::Error> - The service returns a Result type, which is Ok if the operation
/// is successful, or an Err containing an io::Error if an error occurs.
pub fn compress_dir_with_excludes(
    src_dir: &Path,
    dest_zip: &mut Cursor<Vec<u8>>,
    excludes: &[&str],
) -> io::Result<()> {
    let mut zip = ZipWriter::new(dest_zip);
    let options = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    add_dir_to_zip(&mut zip, src_dir, src_dir, options, excludes)?;
    zip.finish()?;

    Ok(())
}

fn add_dir_to_zip<W: Write + io::Seek>(
    zip: &mut ZipWriter<W>,
    src_dir: &Path,
    base_path: &Path,
    options: FileOptions,
    excludes: &[&str],
) -> io::Result<()> {
    for entry in fs::read_dir(src_dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = path.strip_prefix(base_path).unwrap().to_str().unwrap();

        if path.is_dir() && !excludes.contains(&path.file_name().unwrap().to_str().unwrap()) {
            zip.add_directory(name, options)?;
            add_dir_to_zip(zip, &path, base_path, options, excludes)?;
        } else if !excludes.contains(&entry.file_name().to_str().unwrap()) {
            zip.start_file(name, options)?;
            io::copy(&mut File::open(&path)?, zip)?;
        }
    }

    Ok(())
}

pub fn add_dir_to_tar<W: Write>(
    tar: &mut Builder<W>,
    src_dir: &Path,
    base_path: &Path,
    excludes: &[&str],
) -> io::Result<()> {
    for entry in fs::read_dir(src_dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = path.strip_prefix(base_path).unwrap().to_str().unwrap();
        if name == "context.tar" {
            continue;
        }
        if path.is_dir() {
            add_dir_to_tar(tar, &path, base_path, excludes)?;
        } else if !excludes.contains(&entry.file_name().to_str().unwrap()) {
            let mut file = File::open(&path)?;
            let mut header = Header::new_gnu();
            let metadata = file.metadata()?;
            header.set_size(metadata.len());
            header.set_mode(0o644);
            header.set_cksum();

            tar.append_data(&mut header, name, &mut file)?;
        }
    }

    Ok(())
}

pub fn extract_zip_from_cursor(cursor: Cursor<Vec<u8>>, dest_dir: &Path) -> io::Result<()> {
    let mut archive = ZipArchive::new(cursor)?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let file_name = file.name().to_string();

        let out_path = dest_dir.join(file_name);

        if file.is_dir() {
            fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut outfile = File::create(&out_path)?;
            io::copy(&mut file, &mut outfile)?;
        }
    }

    Ok(())
}

pub fn find_file_in_path(file_name: &str, path: &PathBuf) -> Option<String> {
    let dir = fs::read_dir(path).ok()?;
    for entry in dir {
        let entry = entry.ok()?;
        let file = entry.file_name().into_string().ok()?;
        if file == file_name {
            return Some(entry.path().to_str()?.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_camel_case_handler() {
        assert_eq!(to_camel_case_handler("hello-world"), "HelloWorldHandler");
        assert_eq!(to_camel_case_handler("hello"), "HelloHandler");
        assert_eq!(
            to_camel_case_handler("hello-world-again"),
            "HelloWorldAgainHandler"
        );
    }
    #[test]
    fn test_compress_dir_with_excludes() {
        let mut dest_zip = Cursor::new(Vec::new());
        let src_dir = Path::new("test");
        let excludes = ["test.txt"];
        compress_dir_with_excludes(src_dir, &mut dest_zip, &excludes).unwrap();
    }
}
