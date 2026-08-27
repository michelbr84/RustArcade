//! Safe archive extraction and executable discovery.
//!
//! Every entry path is validated before anything touches the disk: no absolute paths, no
//! `..`, no drive prefixes. Symbolic and hard links are never created from archives.
//! Extraction is bounded in total size and entry count to blunt decompression bombs.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use walkdir::WalkDir;

use crate::error::{Error, InstallError, SecurityError};
use crate::platform::Platform;

/// Maximum bytes written by one extraction.
pub const MAX_EXTRACT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Maximum entries in one archive.
pub const MAX_ENTRIES: usize = 20_000;

/// Supported archive formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    Zip,
    TarGz,
    TarXz,
    /// Not an archive: a bare executable.
    Raw,
}

impl ArchiveKind {
    /// Detect from the file name, then confirm with magic bytes.
    pub fn detect(path: &Path) -> Result<ArchiveKind, InstallError> {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let by_name = if name.ends_with(".zip") {
            ArchiveKind::Zip
        } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
            ArchiveKind::TarGz
        } else if name.ends_with(".tar.xz") || name.ends_with(".txz") {
            ArchiveKind::TarXz
        } else if name.ends_with(".tar")
            || name.ends_with(".tar.bz2")
            || name.ends_with(".7z")
            || name.ends_with(".rar")
        {
            return Err(InstallError::UnsupportedArchive { name });
        } else {
            ArchiveKind::Raw
        };
        let mut magic = [0u8; 6];
        let read = File::open(path)
            .and_then(|mut f| f.read(&mut magic))
            .map_err(|e| InstallError::io("read", path, e))?;
        let magic = &magic[..read];
        let ok = match by_name {
            ArchiveKind::Zip => {
                magic.starts_with(b"PK\x03\x04") || magic.starts_with(b"PK\x05\x06")
            }
            ArchiveKind::TarGz => magic.starts_with(&[0x1f, 0x8b]),
            ArchiveKind::TarXz => magic.starts_with(&[0xfd, b'7', b'z', b'X', b'Z', 0x00]),
            ArchiveKind::Raw => true,
        };
        if ok {
            Ok(by_name)
        } else {
            Err(InstallError::UnsupportedArchive {
                name: format!("{name} (content does not match its extension)"),
            })
        }
    }
}

/// Extraction summary.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ExtractReport {
    pub files: usize,
    pub bytes: u64,
    pub skipped_links: usize,
}

/// Validate an archive entry path: relative, normal components only.
pub fn check_entry_path(entry: &str, path: &Path) -> Result<PathBuf, SecurityError> {
    if path.as_os_str().is_empty() {
        return Err(SecurityError::ArchiveEscape {
            entry: entry.to_string(),
        });
    }
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Normal(part) => {
                if part
                    .to_str()
                    .is_some_and(|s| s.contains('\\') || s.contains('\0'))
                {
                    return Err(SecurityError::ArchiveEscape {
                        entry: entry.to_string(),
                    });
                }
                out.push(part);
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(SecurityError::ArchiveEscape {
                    entry: entry.to_string(),
                });
            }
        }
    }
    if out.as_os_str().is_empty() {
        return Err(SecurityError::ArchiveEscape {
            entry: entry.to_string(),
        });
    }
    Ok(out)
}

/// Extract `archive` into `dest` (created if needed) according to its kind. Raw
/// executables are copied into `dest` under their own file name.
pub fn extract(archive: &Path, dest: &Path) -> Result<ExtractReport, Error> {
    fs::create_dir_all(dest).map_err(|e| InstallError::io("create", dest, e))?;
    match ArchiveKind::detect(archive)? {
        ArchiveKind::Zip => extract_zip(archive, dest),
        ArchiveKind::TarGz => {
            let file = File::open(archive).map_err(|e| InstallError::io("open", archive, e))?;
            extract_tar(flate2::read::GzDecoder::new(file), dest)
        }
        ArchiveKind::TarXz => {
            let file = File::open(archive).map_err(|e| InstallError::io("open", archive, e))?;
            extract_tar(liblzma::read::XzDecoder::new(file), dest)
        }
        ArchiveKind::Raw => {
            let name = archive
                .file_name()
                .ok_or_else(|| InstallError::UnsupportedArchive {
                    name: archive.display().to_string(),
                })?;
            let target = dest.join(name);
            let bytes =
                fs::copy(archive, &target).map_err(|e| InstallError::io("copy", &target, e))?;
            make_executable(&target)?;
            Ok(ExtractReport {
                files: 1,
                bytes,
                skipped_links: 0,
            })
        }
    }
}

fn extract_tar<R: Read>(reader: R, dest: &Path) -> Result<ExtractReport, Error> {
    let mut archive = tar::Archive::new(reader);
    archive.set_preserve_permissions(false);
    archive.set_unpack_xattrs(false);
    archive.set_overwrite(true);
    let mut report = ExtractReport::default();
    let entries = archive.entries().map_err(|e| InstallError::Extract {
        entry: "<archive>".into(),
        source: e,
    })?;
    let mut count = 0usize;
    for entry in entries {
        let mut entry = entry.map_err(|e| InstallError::Extract {
            entry: "<archive>".into(),
            source: e,
        })?;
        count += 1;
        if count > MAX_ENTRIES {
            return Err(SecurityError::TooManyEntries { limit: MAX_ENTRIES }.into());
        }
        let raw_path = entry.path().map_err(|e| InstallError::Extract {
            entry: "<entry>".into(),
            source: e,
        })?;
        let entry_name = raw_path.to_string_lossy().to_string();
        let kind = entry.header().entry_type();
        match kind {
            tar::EntryType::Directory => {
                // `./` (the archive root, produced by `tar -C dir .`) is simply the destination.
                if raw_path
                    .components()
                    .all(|c| matches!(c, Component::CurDir))
                {
                    continue;
                }
                let rel = check_entry_path(&entry_name, &raw_path)?;
                let target = dest.join(rel);
                fs::create_dir_all(&target).map_err(|e| InstallError::io("create", &target, e))?;
            }
            tar::EntryType::Regular | tar::EntryType::Continuous | tar::EntryType::GNUSparse => {
                let rel = check_entry_path(&entry_name, &raw_path)?;
                let size = entry.header().size().unwrap_or(0);
                report.bytes = report.bytes.saturating_add(size);
                if report.bytes > MAX_EXTRACT_BYTES {
                    return Err(SecurityError::ArchiveTooLarge {
                        limit_bytes: MAX_EXTRACT_BYTES,
                    }
                    .into());
                }
                if let Some(parent) = dest.join(&rel).parent() {
                    fs::create_dir_all(parent)
                        .map_err(|e| InstallError::io("create", parent, e))?;
                }
                let unpacked = entry.unpack_in(dest).map_err(|e| InstallError::Extract {
                    entry: entry_name.clone(),
                    source: e,
                })?;
                if !unpacked {
                    return Err(SecurityError::ArchiveEscape { entry: entry_name }.into());
                }
                report.files += 1;
            }
            tar::EntryType::Symlink | tar::EntryType::Link => {
                // Links are never materialised from untrusted archives.
                tracing::warn!("skipping link entry `{entry_name}` in archive");
                report.skipped_links += 1;
            }
            _ => {
                tracing::debug!("skipping special entry `{entry_name}` ({kind:?})");
            }
        }
    }
    Ok(report)
}

fn extract_zip(archive_path: &Path, dest: &Path) -> Result<ExtractReport, Error> {
    let file = File::open(archive_path).map_err(|e| InstallError::io("open", archive_path, e))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| InstallError::Extract {
        entry: "<archive>".into(),
        source: io::Error::other(e.to_string()),
    })?;
    if archive.len() > MAX_ENTRIES {
        return Err(SecurityError::TooManyEntries { limit: MAX_ENTRIES }.into());
    }
    let mut report = ExtractReport::default();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| InstallError::Extract {
            entry: format!("#{i}"),
            source: io::Error::other(e.to_string()),
        })?;
        let entry_name = entry.name().to_string();
        let Some(enclosed) = entry.enclosed_name() else {
            return Err(SecurityError::ArchiveEscape { entry: entry_name }.into());
        };
        let rel = check_entry_path(&entry_name, &enclosed)?;
        if entry.is_symlink() {
            tracing::warn!("skipping symlink entry `{entry_name}` in archive");
            report.skipped_links += 1;
            continue;
        }
        let target = dest.join(&rel);
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|e| InstallError::io("create", &target, e))?;
            continue;
        }
        report.bytes = report.bytes.saturating_add(entry.size());
        if report.bytes > MAX_EXTRACT_BYTES {
            return Err(SecurityError::ArchiveTooLarge {
                limit_bytes: MAX_EXTRACT_BYTES,
            }
            .into());
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| InstallError::io("create", parent, e))?;
        }
        let mut out = File::create(&target).map_err(|e| InstallError::io("create", &target, e))?;
        let mut limited = (&mut entry).take(MAX_EXTRACT_BYTES);
        io::copy(&mut limited, &mut out).map_err(|e| InstallError::Extract {
            entry: entry_name.clone(),
            source: e,
        })?;
        report.files += 1;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode()
            && mode & 0o111 != 0
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&target, fs::Permissions::from_mode(0o755));
        }
    }
    Ok(report)
}

/// Locate the game executable inside `root`.
///
/// `binary` is either a bare name (searched up to 5 levels deep, shallowest wins) or a
/// relative path that must exist exactly. On Windows `name.exe` is accepted for `name`.
pub fn discover_binary(
    root: &Path,
    binary: &Path,
    platform: &Platform,
) -> Result<PathBuf, InstallError> {
    let expected = binary.display().to_string();
    let file_name = binary
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(&expected)
        .to_string();
    let candidates_names: Vec<String> = if platform.exe_suffix().is_empty() {
        vec![file_name.clone()]
    } else {
        vec![
            format!("{file_name}{}", platform.exe_suffix()),
            file_name.clone(),
        ]
    };

    if binary.parent().is_some_and(|p| !p.as_os_str().is_empty()) {
        for name in &candidates_names {
            let path = root.join(binary.with_file_name(name));
            if path.is_file() {
                return Ok(path);
            }
        }
    } else {
        let mut best: Option<(usize, PathBuf)> = None;
        let mut ambiguous = false;
        for entry in WalkDir::new(root)
            .max_depth(5)
            .follow_links(false)
            .into_iter()
            .flatten()
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy();
            let matches = candidates_names.iter().any(|c| {
                if cfg!(windows) {
                    c.eq_ignore_ascii_case(&name)
                } else {
                    c == &*name
                }
            });
            if !matches {
                continue;
            }
            let depth = entry.depth();
            match &best {
                Some((d, _)) if depth > *d => {}
                Some((d, _)) if depth == *d => ambiguous = true,
                _ => {
                    best = Some((depth, entry.path().to_path_buf()));
                    ambiguous = false;
                }
            }
        }
        if let Some((_, path)) = best {
            if ambiguous {
                return Err(InstallError::AmbiguousAsset {
                    candidates: list_files(root),
                });
            }
            return Ok(path);
        }
        // Fallback: exactly one executable-looking file, or exactly one file at all
        // (raw binary assets); the caller still verifies the executable format.
        let all_files: Vec<PathBuf> = WalkDir::new(root)
            .max_depth(5)
            .follow_links(false)
            .into_iter()
            .flatten()
            .filter(|e| e.file_type().is_file())
            .map(|e| e.path().to_path_buf())
            .collect();
        let executables: Vec<&PathBuf> = all_files.iter().filter(|p| looks_executable(p)).collect();
        if executables.len() == 1 {
            tracing::info!(
                "using sole executable {} for expected `{expected}`",
                executables[0].display()
            );
            return Ok(executables[0].clone());
        }
        if all_files.len() == 1 && !looks_like_document(&all_files[0]) {
            tracing::info!(
                "using sole file {} for expected `{expected}`",
                all_files[0].display()
            );
            return Ok(all_files[0].clone());
        }
    }
    Err(InstallError::BinaryNotFound {
        expected,
        found: list_files(root),
    })
}

fn looks_like_document(path: &Path) -> bool {
    const DOC_EXTS: &[&str] = &[
        "md", "txt", "html", "htm", "json", "toml", "yml", "yaml", "sha256", "sha512", "sig",
        "asc", "pem", "rst", "license", "pdf", "png", "gif", "jpg", "svg", "sbom", "spdx",
    ];
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| DOC_EXTS.contains(&e.to_ascii_lowercase().as_str()))
}

fn looks_executable(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if cfg!(windows) {
        return name.to_ascii_lowercase().ends_with(".exe");
    }
    if name.contains('.') {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        false
    }
}

fn list_files(root: &Path) -> Vec<String> {
    WalkDir::new(root)
        .max_depth(5)
        .follow_links(false)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .take(20)
        .map(|e| {
            e.path()
                .strip_prefix(root)
                .unwrap_or(e.path())
                .display()
                .to_string()
        })
        .collect()
}

/// Check that `path` is a plausible native executable (size > 0, known magic bytes).
pub fn verify_executable(path: &Path) -> Result<(), InstallError> {
    let meta = fs::metadata(path).map_err(|e| InstallError::io("stat", path, e))?;
    if !meta.is_file() {
        return Err(InstallError::NotAnExecutable {
            path: path.to_path_buf(),
            reason: "not a regular file".into(),
        });
    }
    if meta.len() == 0 {
        return Err(InstallError::NotAnExecutable {
            path: path.to_path_buf(),
            reason: "file is empty".into(),
        });
    }
    let mut magic = [0u8; 4];
    let n = File::open(path)
        .and_then(|mut f| f.read(&mut magic))
        .map_err(|e| InstallError::io("read", path, e))?;
    let magic = &magic[..n];
    let is_elf = magic.starts_with(b"\x7fELF");
    let is_pe = magic.starts_with(b"MZ");
    let is_macho = matches!(
        magic,
        [0xcf, 0xfa, 0xed, 0xfe]
            | [0xce, 0xfa, 0xed, 0xfe]
            | [0xca, 0xfe, 0xba, 0xbe]
            | [0xfe, 0xed, 0xfa, 0xcf]
            | [0xfe, 0xed, 0xfa, 0xce]
    );
    let is_script = magic.starts_with(b"#!");
    if is_elf || is_pe || is_macho || is_script {
        Ok(())
    } else {
        Err(InstallError::NotAnExecutable {
            path: path.to_path_buf(),
            reason: "not a recognised executable format (expected ELF, Mach-O or PE)".into(),
        })
    }
}

/// Mark a file executable (no-op on Windows).
pub fn make_executable(path: &Path) -> Result<(), InstallError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .map_err(|e| InstallError::io("chmod", path, e))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::platform::{Arch, Os};
    use std::io::Write;

    pub(crate) fn fake_elf() -> Vec<u8> {
        let mut v = b"\x7fELF".to_vec();
        v.extend_from_slice(&[0u8; 60]);
        v
    }

    pub(crate) fn make_tar_gz(entries: &[(&str, &[u8], u32)]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (name, data, mode) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(*mode);
            header.set_entry_type(tar::EntryType::Regular);
            set_raw_name(&mut header, name);
            header.set_cksum();
            builder.append(&header, *data).unwrap();
        }
        let tar = builder.into_inner().unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&tar).unwrap();
        gz.finish().unwrap()
    }

    pub(crate) fn make_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut cursor = io::Cursor::new(Vec::new());
        {
            let mut zw = zip::ZipWriter::new(&mut cursor);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .unix_permissions(0o755);
            for (name, data) in entries {
                zw.start_file(*name, opts).unwrap();
                zw.write_all(data).unwrap();
            }
            zw.finish().unwrap();
        }
        cursor.into_inner()
    }

    fn set_raw_name(header: &mut tar::Header, name: &str) {
        let gnu = header.as_gnu_mut().unwrap();
        let bytes = name.as_bytes();
        gnu.name[..bytes.len()].copy_from_slice(bytes);
    }

    fn tar_with_link(kind: tar::EntryType, name: &str, target: &str) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(0);
        header.set_mode(0o777);
        header.set_entry_type(kind);
        set_raw_name(&mut header, name);
        let bytes = target.as_bytes();
        header.as_gnu_mut().unwrap().linkname[..bytes.len()].copy_from_slice(bytes);
        header.set_cksum();
        builder.append(&header, io::empty()).unwrap();
        let tar = builder.into_inner().unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&tar).unwrap();
        gz.finish().unwrap()
    }

    fn linux() -> Platform {
        Platform::new(Os::Linux, Arch::X86_64)
    }

    #[test]
    fn detects_kinds_and_rejects_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let tgz = dir.path().join("a.tar.gz");
        fs::write(&tgz, make_tar_gz(&[("x", b"1", 0o644)])).unwrap();
        assert_eq!(ArchiveKind::detect(&tgz).unwrap(), ArchiveKind::TarGz);
        let zipf = dir.path().join("a.zip");
        fs::write(&zipf, make_zip(&[("x", b"1")])).unwrap();
        assert_eq!(ArchiveKind::detect(&zipf).unwrap(), ArchiveKind::Zip);
        let fake = dir.path().join("fake.zip");
        fs::write(&fake, b"<html>not a zip</html>").unwrap();
        assert!(matches!(
            ArchiveKind::detect(&fake),
            Err(InstallError::UnsupportedArchive { .. })
        ));
        let raw = dir.path().join("game");
        fs::write(&raw, fake_elf()).unwrap();
        assert_eq!(ArchiveKind::detect(&raw).unwrap(), ArchiveKind::Raw);
        let seven = dir.path().join("a.7z");
        fs::write(&seven, b"7z").unwrap();
        assert!(ArchiveKind::detect(&seven).is_err());
    }

    #[test]
    fn extracts_tar_gz_and_zip_with_nested_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let tgz = dir.path().join("g.tar.gz");
        fs::write(
            &tgz,
            make_tar_gz(&[
                ("game-1.0/game", &fake_elf(), 0o755),
                ("game-1.0/README", b"hi", 0o644),
            ]),
        )
        .unwrap();
        let out = dir.path().join("out-tar");
        let report = extract(&tgz, &out).unwrap();
        assert_eq!(report.files, 2);
        assert!(out.join("game-1.0/game").is_file());
        let found = discover_binary(&out, Path::new("game"), &linux()).unwrap();
        assert_eq!(found, out.join("game-1.0/game"));

        let zipf = dir.path().join("g.zip");
        fs::write(
            &zipf,
            make_zip(&[("game.exe", &fake_elf()), ("docs/readme.txt", b"x")]),
        )
        .unwrap();
        let out2 = dir.path().join("out-zip");
        extract(&zipf, &out2).unwrap();
        assert!(out2.join("docs/readme.txt").is_file());
        let win = Platform::new(Os::Windows, Arch::X86_64);
        assert_eq!(
            discover_binary(&out2, Path::new("game"), &win).unwrap(),
            out2.join("game.exe")
        );
    }

    #[test]
    fn tar_created_with_dot_root_extracts() {
        // `tar czf x.tar.gz -C dir .` produces `./` and `./file` entries.
        let dir = tempfile::tempdir().unwrap();
        let mut builder = tar::Builder::new(Vec::new());
        let mut root = tar::Header::new_gnu();
        root.set_size(0);
        root.set_mode(0o755);
        root.set_entry_type(tar::EntryType::Directory);
        set_raw_name(&mut root, "./");
        root.set_cksum();
        builder.append(&root, io::empty()).unwrap();
        let mut file = tar::Header::new_gnu();
        file.set_size(4);
        file.set_mode(0o755);
        file.set_entry_type(tar::EntryType::Regular);
        set_raw_name(&mut file, "./hammurabi");
        file.set_cksum();
        builder.append(&file, &b"data"[..]).unwrap();
        let tar = builder.into_inner().unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&tar).unwrap();
        let path = dir.path().join("h.tar.gz");
        fs::write(&path, gz.finish().unwrap()).unwrap();
        let out = dir.path().join("out");
        let r = extract(&path, &out).unwrap();
        assert_eq!(r.files, 1);
        assert_eq!(fs::read(out.join("hammurabi")).unwrap(), b"data");
    }

    #[test]
    fn extracts_tar_xz() {
        let dir = tempfile::tempdir().unwrap();
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(4);
        header.set_mode(0o755);
        header.set_entry_type(tar::EntryType::Regular);
        set_raw_name(&mut header, "bin/tool");
        header.set_cksum();
        builder.append(&header, &b"data"[..]).unwrap();
        let tar = builder.into_inner().unwrap();
        let mut xz = liblzma::write::XzEncoder::new(Vec::new(), 1);
        xz.write_all(&tar).unwrap();
        let bytes = xz.finish().unwrap();
        let path = dir.path().join("t.tar.xz");
        fs::write(&path, bytes).unwrap();
        let out = dir.path().join("out");
        let r = extract(&path, &out).unwrap();
        assert_eq!(r.files, 1);
        assert_eq!(fs::read(out.join("bin/tool")).unwrap(), b"data");
    }

    #[test]
    fn rejects_traversal_and_absolute_entries() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["../evil", "/abs/evil", "a/../../evil"] {
            let tgz = dir.path().join("bad.tar.gz");
            fs::write(&tgz, make_tar_gz(&[(name, b"x", 0o644)])).unwrap();
            let out = dir.path().join("out");
            let err = extract(&tgz, &out).unwrap_err();
            assert!(err.is_security(), "{name}: {err}");
            assert!(!dir.path().join("evil").exists());
        }
        let zipf = dir.path().join("bad.zip");
        fs::write(&zipf, make_zip(&[("../evil.txt", b"x")])).unwrap();
        let err = extract(&zipf, &dir.path().join("outz")).unwrap_err();
        assert!(err.is_security(), "{err}");
        assert!(!dir.path().join("evil.txt").exists());
    }

    #[test]
    fn links_are_skipped_never_created() {
        let dir = tempfile::tempdir().unwrap();
        let sym = dir.path().join("sym.tar.gz");
        fs::write(
            &sym,
            tar_with_link(tar::EntryType::Symlink, "link", "/etc/passwd"),
        )
        .unwrap();
        let out = dir.path().join("out-sym");
        let r = extract(&sym, &out).unwrap();
        assert_eq!(r.skipped_links, 1);
        assert!(!out.join("link").exists());
        let hard = dir.path().join("hard.tar.gz");
        fs::write(
            &hard,
            tar_with_link(tar::EntryType::Link, "hl", "../../secret"),
        )
        .unwrap();
        let r = extract(&hard, &dir.path().join("out-hard")).unwrap();
        assert_eq!(r.skipped_links, 1);
    }

    #[test]
    fn entry_limit_is_enforced() {
        let dir = tempfile::tempdir().unwrap();
        let names: Vec<String> = (0..(MAX_ENTRIES + 1)).map(|i| format!("f{i}")).collect();
        let entries: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), &b""[..])).collect();
        let zipf = dir.path().join("many.zip");
        fs::write(&zipf, make_zip(&entries)).unwrap();
        let err = extract(&zipf, &dir.path().join("out")).unwrap_err();
        assert!(matches!(
            err,
            Error::Security(SecurityError::TooManyEntries { .. })
        ));
    }

    #[test]
    fn raw_asset_is_copied() {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join("termfarm_Linux_x86_64");
        fs::write(&raw, fake_elf()).unwrap();
        let out = dir.path().join("out");
        extract(&raw, &out).unwrap();
        assert!(out.join("termfarm_Linux_x86_64").is_file());
        // Discovery falls back to the sole executable-looking file on unix.
        #[cfg(unix)]
        {
            make_executable(&out.join("termfarm_Linux_x86_64")).unwrap();
            let found = discover_binary(&out, Path::new("termfarm"), &linux()).unwrap();
            assert_eq!(found, out.join("termfarm_Linux_x86_64"));
        }
    }

    #[test]
    fn discovery_errors_list_contents() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("README.md"), "x").unwrap();
        let err = discover_binary(dir.path(), Path::new("game"), &linux()).unwrap_err();
        match err {
            InstallError::BinaryNotFound { expected, found } => {
                assert_eq!(expected, "game");
                assert_eq!(found, vec!["README.md"]);
            }
            other => panic!("{other:?}"),
        }
        fs::create_dir_all(dir.path().join("a")).unwrap();
        fs::create_dir_all(dir.path().join("b")).unwrap();
        fs::write(dir.path().join("a/game"), "1").unwrap();
        fs::write(dir.path().join("b/game"), "2").unwrap();
        assert!(matches!(
            discover_binary(dir.path(), Path::new("game"), &linux()),
            Err(InstallError::AmbiguousAsset { .. })
        ));
        fs::write(dir.path().join("game"), "root").unwrap();
        assert_eq!(
            discover_binary(dir.path(), Path::new("game"), &linux()).unwrap(),
            dir.path().join("game")
        );
        assert_eq!(
            discover_binary(dir.path(), Path::new("b/game"), &linux()).unwrap(),
            dir.path().join("b/game")
        );
    }

    #[test]
    fn verify_executable_checks_magic() {
        let dir = tempfile::tempdir().unwrap();
        let elf = dir.path().join("elf");
        fs::write(&elf, fake_elf()).unwrap();
        verify_executable(&elf).unwrap();
        let html = dir.path().join("html");
        fs::write(&html, b"<!doctype html>").unwrap();
        assert!(matches!(
            verify_executable(&html),
            Err(InstallError::NotAnExecutable { .. })
        ));
        let empty = dir.path().join("empty");
        fs::write(&empty, b"").unwrap();
        assert!(verify_executable(&empty).is_err());
        let pe = dir.path().join("pe");
        fs::write(&pe, b"MZ\x90\x00").unwrap();
        verify_executable(&pe).unwrap();
        let macho = dir.path().join("macho");
        fs::write(&macho, [0xcf, 0xfa, 0xed, 0xfe, 0, 0]).unwrap();
        verify_executable(&macho).unwrap();
    }

    #[test]
    fn entry_path_rules() {
        assert!(check_entry_path("x", Path::new("a/b")).is_ok());
        assert!(check_entry_path("x", Path::new("./a")).is_ok());
        assert!(check_entry_path("x", Path::new("")).is_err());
        assert!(check_entry_path("x", Path::new("../a")).is_err());
        assert!(check_entry_path("x", Path::new("/a")).is_err());
        // Backslashes are ordinary file-name characters on Unix (so `a\b` is one suspicious
        // component) but path separators on Windows (so it is the safe path `a/b`).
        #[cfg(unix)]
        assert!(check_entry_path("x", Path::new("a\\b")).is_err());
        #[cfg(windows)]
        assert_eq!(
            check_entry_path("x", Path::new("a\\b")).unwrap(),
            PathBuf::from("a").join("b")
        );
    }
}
