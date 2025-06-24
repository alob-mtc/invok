use std::fs::{self, File};
use std::io::{self, Cursor, Write};
use std::path::{Path, PathBuf};
use tar::{Builder, Header};
use zip::write::FileOptions;
use zip::{ZipArchive, ZipWriter};
pub mod go_template;
pub mod nodejs_template;
