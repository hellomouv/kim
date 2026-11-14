//! Build-time helper for Kim's Windows executables: writes a COFF object with a `.rsrc` section
//! holding the program icon (`RT_ICON` + `RT_GROUP_ICON`, from an `.ico` file) and version
//! information (`RT_VERSION`: description, product, version), and tells Cargo to link it into
//! the package's binaries. It needs neither `rc.exe` nor `windres`, so the GNU and MSVC
//! toolchains build the same icon (Explorer, the taskbar, shortcuts and Task Manager show it).
//!
//! The object is what `cvtres`/`windres` would produce: one resource directory tree (type →
//! id → language), data entries whose RVAs are `ADDR32NB` relocations against the section
//! symbol, and the resource bytes. [`resources`] reads the tree back out of a finished `.exe`
//! (tests check that the shipped programs carry their icon).
//!
//! Nothing here runs in a shipped program; it is a build dependency only.

use std::path::{Path, PathBuf};

/// `RT_ICON`.
pub const RT_ICON: u16 = 3;
/// `RT_GROUP_ICON`.
pub const RT_GROUP_ICON: u16 = 14;
/// `RT_VERSION`.
pub const RT_VERSION: u16 = 16;
/// The id of the program icon group (the first group is the one Explorer shows).
pub const ICON_GROUP_ID: u16 = 1;
/// en-US: the language of every resource written here.
pub const LANG: u16 = 0x0409;

/// The target machine of the object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Machine {
    X86_64,
    Aarch64,
    X86,
}

impl Machine {
    /// From Cargo's `CARGO_CFG_TARGET_ARCH`.
    pub fn from_arch(arch: &str) -> Option<Self> {
        match arch {
            "x86_64" => Some(Self::X86_64),
            "aarch64" => Some(Self::Aarch64),
            "x86" => Some(Self::X86),
            _ => None,
        }
    }
    fn id(self) -> u16 {
        match self {
            Self::X86_64 => 0x8664,
            Self::Aarch64 => 0xAA64,
            Self::X86 => 0x014C,
        }
    }
    /// The image-relative 32-bit relocation of this machine.
    fn addr32nb(self) -> u16 {
        match self {
            Self::X86_64 => 0x0003,  // IMAGE_REL_AMD64_ADDR32NB
            Self::Aarch64 => 0x0002, // IMAGE_REL_ARM64_ADDR32NB
            Self::X86 => 0x0007,     // IMAGE_REL_I386_DIR32NB
        }
    }
}

/// The version information Explorer and Task Manager show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionInfo {
    /// `FileDescription`: the name Task Manager lists (e.g. "Kim").
    pub description: String,
    /// `ProductName`.
    pub product: String,
    /// `FileVersion`/`ProductVersion`, `major.minor.patch` (a pre-release suffix is kept in
    /// the strings and dropped from the numbers).
    pub version: String,
    /// `OriginalFilename`/`InternalName`, e.g. `kim.exe`.
    pub file_name: String,
}

/// One image of an `.ico` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconImage {
    /// The 12-byte directory fields shared by `.ico` and `RT_GROUP_ICON` (width, height, colours,
    /// reserved, planes, bit count, size).
    pub header: [u8; 12],
    pub data: Vec<u8>,
}

/// Reads the images of an `.ico` file.
pub fn parse_ico(ico: &[u8]) -> Result<Vec<IconImage>, String> {
    let u16_at = |o: usize| -> Result<u16, String> {
        ico.get(o..o + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .ok_or_else(|| "the .ico file is truncated".to_string())
    };
    let u32_at = |o: usize| -> Result<u32, String> {
        ico.get(o..o + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or_else(|| "the .ico file is truncated".to_string())
    };
    if u16_at(0)? != 0 || u16_at(2)? != 1 {
        return Err("not an .ico file (ICONDIR reserved 0, type 1)".into());
    }
    let n = usize::from(u16_at(4)?);
    if n == 0 {
        return Err("the .ico file has no images".into());
    }
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let e = 6 + 16 * i;
        let entry = ico
            .get(e..e + 16)
            .ok_or_else(|| "the .ico directory is truncated".to_string())?;
        let size = u32_at(e + 8)? as usize;
        let offset = u32_at(e + 12)? as usize;
        let data = ico
            .get(offset..offset.checked_add(size).ok_or("bad .ico image size")?)
            .ok_or_else(|| format!("image {i} of the .ico file lies outside the file"))?;
        let mut header = [0u8; 12];
        header.copy_from_slice(&entry[..12]);
        out.push(IconImage {
            header,
            data: data.to_vec(),
        });
    }
    Ok(out)
}

/// The `RT_GROUP_ICON` resource for `images` stored as `RT_ICON` ids `1..=n`.
pub fn group_icon(images: &[IconImage]) -> Vec<u8> {
    let mut g = Vec::with_capacity(6 + 14 * images.len());
    g.extend_from_slice(&0u16.to_le_bytes());
    g.extend_from_slice(&1u16.to_le_bytes());
    g.extend_from_slice(&(images.len() as u16).to_le_bytes());
    for (i, im) in images.iter().enumerate() {
        g.extend_from_slice(&im.header);
        g.extend_from_slice(&(i as u16 + 1).to_le_bytes());
    }
    g
}

fn utf16z(s: &str) -> Vec<u8> {
    s.encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}

fn pad4(v: &mut Vec<u8>) {
    while !v.len().is_multiple_of(4) {
        v.push(0);
    }
}

/// One `VS_VERSIONINFO` node: header, key, value, children (each 4-byte aligned).
fn node(key: &str, value: &[u8], value_len: u16, text: bool, children: &[Vec<u8>]) -> Vec<u8> {
    let mut n = vec![0u8; 6];
    n[2..4].copy_from_slice(&value_len.to_le_bytes());
    n[4..6].copy_from_slice(&u16::from(text).to_le_bytes());
    n.extend(utf16z(key));
    pad4(&mut n);
    n.extend_from_slice(value);
    for c in children {
        pad4(&mut n);
        n.extend_from_slice(c);
    }
    let len = n.len() as u16;
    n[0..2].copy_from_slice(&len.to_le_bytes());
    n
}

/// The numeric `a.b.c.0` of a version string (`0.0.1`, `1.2.3-preview.4`).
pub fn version_numbers(v: &str) -> [u16; 4] {
    let core = v.split(['-', '+']).next().unwrap_or("");
    let mut out = [0u16; 4];
    for (i, p) in core.split('.').take(3).enumerate() {
        out[i] = p.parse().unwrap_or(0);
    }
    out
}

/// The `RT_VERSION` resource.
pub fn version_resource(info: &VersionInfo) -> Vec<u8> {
    let [a, b, c, d] = version_numbers(&info.version);
    let ms = (u32::from(a) << 16) | u32::from(b);
    let ls = (u32::from(c) << 16) | u32::from(d);
    let mut fixed = Vec::with_capacity(52);
    for v in [
        0xFEEF_04BD_u32, // signature
        0x0001_0000,     // structure version
        ms,
        ls,
        ms,
        ls,
        0x3F,        // flags mask
        0,           // flags
        0x0004_0004, // VOS_NT_WINDOWS32
        1,           // VFT_APP
        0,
        0,
        0,
    ] {
        fixed.extend_from_slice(&v.to_le_bytes());
    }
    let string = |k: &str, v: &str| {
        let value = utf16z(v);
        node(k, &value, (value.len() / 2) as u16, true, &[])
    };
    let strings = [
        string("CompanyName", "mouv"),
        string("FileDescription", &info.description),
        string("FileVersion", &info.version),
        string("InternalName", &info.file_name),
        string("LegalCopyright", "MIT OR Apache-2.0"),
        string("OriginalFilename", &info.file_name),
        string("ProductName", &info.product),
        string("ProductVersion", &info.version),
    ];
    let table = node("040904b0", &[], 0, true, &strings);
    let string_info = node("StringFileInfo", &[], 0, true, &[table]);
    let translation = 0x04B0_0409_u32.to_le_bytes(); // en-US, Unicode
    let var = node("Translation", &translation, 4, false, &[]);
    let var_info = node("VarFileInfo", &[], 0, true, &[var]);
    node(
        "VS_VERSION_INFO",
        &fixed,
        52,
        false,
        &[string_info, var_info],
    )
}

/// The COFF object: a `.rsrc` section with `entries` (type, id, data), all in [`LANG`].
pub fn object(machine: Machine, entries: &[(u16, u16, Vec<u8>)]) -> Vec<u8> {
    let mut entries: Vec<&(u16, u16, Vec<u8>)> = entries.iter().collect();
    entries.sort_by_key(|(t, id, _)| (*t, *id));
    let mut types: Vec<u16> = entries.iter().map(|e| e.0).collect();
    types.dedup();

    // Layout: root directory, one id directory per type, one language directory per resource,
    // then the data entries, then the data (8-byte aligned).
    let dir_len = |n: usize| 16 + 8 * n;
    let root_len = dir_len(types.len());
    let type_dirs: Vec<usize> = types
        .iter()
        .map(|t| dir_len(entries.iter().filter(|e| e.0 == *t).count()))
        .collect();
    let lang_dirs_at = root_len + type_dirs.iter().sum::<usize>();
    let data_entries_at = lang_dirs_at + entries.len() * dir_len(1);
    let mut data_at = data_entries_at + entries.len() * 16;
    data_at = data_at.next_multiple_of(8);
    let mut data_offsets = Vec::with_capacity(entries.len());
    let mut end = data_at;
    for e in &entries {
        data_offsets.push(end);
        end = (end + e.2.len()).next_multiple_of(8);
    }

    let mut s = vec![0u8; end];
    let put16 =
        |s: &mut Vec<u8>, at: usize, v: u16| s[at..at + 2].copy_from_slice(&v.to_le_bytes());
    let put32 =
        |s: &mut Vec<u8>, at: usize, v: u32| s[at..at + 4].copy_from_slice(&v.to_le_bytes());
    let dir = |s: &mut Vec<u8>, at: usize, ids: usize| put16(s, at + 14, ids as u16);
    const SUBDIR: u32 = 0x8000_0000;

    dir(&mut s, 0, types.len());
    let mut type_at = root_len;
    let mut res = 0usize;
    let mut relocs = Vec::with_capacity(entries.len());
    for (ti, t) in types.iter().enumerate() {
        put32(&mut s, 16 + 8 * ti, u32::from(*t));
        put32(&mut s, 16 + 8 * ti + 4, SUBDIR | type_at as u32);
        let of_type: Vec<usize> = (0..entries.len()).filter(|i| entries[*i].0 == *t).collect();
        dir(&mut s, type_at, of_type.len());
        for (k, i) in of_type.iter().enumerate() {
            let lang_at = lang_dirs_at + res * dir_len(1);
            put32(&mut s, type_at + 16 + 8 * k, u32::from(entries[*i].1));
            put32(&mut s, type_at + 16 + 8 * k + 4, SUBDIR | lang_at as u32);
            dir(&mut s, lang_at, 1);
            let data_entry = data_entries_at + res * 16;
            put32(&mut s, lang_at + 16, u32::from(LANG));
            put32(&mut s, lang_at + 20, data_entry as u32);
            // OffsetToData is an RVA: the section-relative offset here, fixed up by the linker.
            put32(&mut s, data_entry, data_offsets[*i] as u32);
            put32(&mut s, data_entry + 4, entries[*i].2.len() as u32);
            relocs.push(data_entry as u32);
            let d = data_offsets[*i];
            s[d..d + entries[*i].2.len()].copy_from_slice(&entries[*i].2);
            res += 1;
        }
        type_at += type_dirs[ti];
    }

    // The object file: header, one section header, raw data, relocations, symbols, strings.
    let header_len = 20 + 40;
    let raw_at = header_len;
    let relocs_at = raw_at + s.len();
    let symbols_at = relocs_at + relocs.len() * 10;
    let mut o = Vec::with_capacity(symbols_at + 36 + 4);
    o.extend_from_slice(&machine.id().to_le_bytes());
    o.extend_from_slice(&1u16.to_le_bytes()); // sections
    o.extend_from_slice(&0u32.to_le_bytes()); // timestamp: 0, reproducible
    o.extend_from_slice(&(symbols_at as u32).to_le_bytes());
    o.extend_from_slice(&2u32.to_le_bytes()); // the section symbol and its aux record
    o.extend_from_slice(&0u16.to_le_bytes()); // no optional header
    o.extend_from_slice(&0u16.to_le_bytes());
    o.extend_from_slice(b".rsrc\0\0\0");
    o.extend_from_slice(&0u32.to_le_bytes()); // virtual size
    o.extend_from_slice(&0u32.to_le_bytes()); // virtual address
    o.extend_from_slice(&(s.len() as u32).to_le_bytes());
    o.extend_from_slice(&(raw_at as u32).to_le_bytes());
    o.extend_from_slice(&(relocs_at as u32).to_le_bytes());
    o.extend_from_slice(&0u32.to_le_bytes()); // line numbers
    o.extend_from_slice(&(relocs.len() as u16).to_le_bytes());
    o.extend_from_slice(&0u16.to_le_bytes());
    // INITIALIZED_DATA | ALIGN_8BYTES | MEM_READ
    o.extend_from_slice(&0x4040_0040_u32.to_le_bytes());
    o.extend_from_slice(&s);
    for r in &relocs {
        o.extend_from_slice(&r.to_le_bytes());
        o.extend_from_slice(&0u32.to_le_bytes()); // symbol 0: the section
        o.extend_from_slice(&machine.addr32nb().to_le_bytes());
    }
    // Symbol 0: `.rsrc`, static, section 1, with a section-definition aux record.
    o.extend_from_slice(b".rsrc\0\0\0");
    o.extend_from_slice(&0u32.to_le_bytes());
    o.extend_from_slice(&1u16.to_le_bytes());
    o.extend_from_slice(&0u16.to_le_bytes());
    o.push(3); // IMAGE_SYM_CLASS_STATIC
    o.push(1);
    o.extend_from_slice(&(s.len() as u32).to_le_bytes());
    o.extend_from_slice(&(relocs.len() as u16).to_le_bytes());
    o.extend_from_slice(&[0u8; 12]);
    o.extend_from_slice(&4u32.to_le_bytes()); // empty string table
    o
}

/// The resources of a program icon and version information.
pub fn program_resources(
    ico: &[u8],
    info: &VersionInfo,
) -> Result<Vec<(u16, u16, Vec<u8>)>, String> {
    let images = parse_ico(ico)?;
    let mut entries: Vec<(u16, u16, Vec<u8>)> = images
        .iter()
        .enumerate()
        .map(|(i, im)| (RT_ICON, i as u16 + 1, im.data.clone()))
        .collect();
    entries.push((RT_GROUP_ICON, ICON_GROUP_ID, group_icon(&images)));
    entries.push((RT_VERSION, 1, version_resource(info)));
    Ok(entries)
}

/// For a `build.rs`: links `icon` (relative to the package) and version information into every
/// binary of the package. Does nothing when the target is not Windows.
///
/// # Panics
/// When the icon cannot be read or the object cannot be written (the build fails with the
/// reason).
pub fn embed(icon: &str, description: &str) {
    let manifest_dir =
        PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let path = manifest_dir.join(icon);
    println!("cargo:rerun-if-changed={}", path.display());
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let Some(machine) = Machine::from_arch(&arch) else {
        println!("cargo:warning=kim-winres: no icon for target arch {arch}");
        return;
    };
    let ico =
        std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let name = std::env::var("CARGO_PKG_NAME").unwrap_or_default();
    let info = VersionInfo {
        description: description.to_string(),
        product: "Kim".to_string(),
        version: std::env::var("CARGO_PKG_VERSION").unwrap_or_default(),
        file_name: format!("{name}.exe"),
    };
    let entries =
        program_resources(&ico, &info).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR")).join("kim-resources.o");
    write(&out, &object(machine, &entries));
    println!("cargo:rustc-link-arg-bins={}", out.display());
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::write(path, bytes).unwrap_or_else(|e| panic!("cannot write {}: {e}", path.display()));
}

/// Reads the resources (type, id, data) of a finished PE file (`.exe`/`.dll`): for tests that
/// check what a program carries.
pub fn resources(pe: &[u8]) -> Result<Vec<(u16, u16, Vec<u8>)>, String> {
    let u16_at = |o: usize| pe.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let u32_at = |o: usize| {
        pe.get(o..o + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let bad = || "not a readable PE file".to_string();
    let pe_at = u32_at(0x3C).ok_or_else(bad)? as usize;
    if pe.get(pe_at..pe_at + 4) != Some(b"PE\0\0") {
        return Err(bad());
    }
    let fh = pe_at + 4;
    let sections = usize::from(u16_at(fh + 2).ok_or_else(bad)?);
    let opt_len = usize::from(u16_at(fh + 16).ok_or_else(bad)?);
    let opt = fh + 20;
    let dirs = match u16_at(opt).ok_or_else(bad)? {
        0x20B => opt + 112,
        0x10B => opt + 96,
        _ => return Err(bad()),
    };
    let rsrc_rva = u32_at(dirs + 2 * 8).ok_or_else(bad)?;
    if rsrc_rva == 0 {
        return Ok(Vec::new());
    }
    let sh = opt + opt_len;
    let mut map = None;
    for i in 0..sections {
        let s = sh + 40 * i;
        let va = u32_at(s + 12).ok_or_else(bad)?;
        let raw_len = u32_at(s + 16).ok_or_else(bad)?;
        let raw = u32_at(s + 20).ok_or_else(bad)?;
        let vlen = u32_at(s + 8).ok_or_else(bad)?.max(raw_len);
        if rsrc_rva >= va && rsrc_rva < va + vlen {
            map = Some((va, raw));
        }
    }
    let (va, raw) = map.ok_or_else(bad)?;
    let base = (raw + (rsrc_rva - va)) as usize;
    let file = |rva: u32| (raw + (rva - va)) as usize;
    let entries = |at: usize| -> Result<Vec<(u32, u32)>, String> {
        let named = usize::from(u16_at(at + 12).ok_or_else(bad)?);
        let ids = usize::from(u16_at(at + 14).ok_or_else(bad)?);
        (0..named + ids)
            .map(|k| {
                let e = at + 16 + 8 * k;
                Ok((u32_at(e).ok_or_else(bad)?, u32_at(e + 4).ok_or_else(bad)?))
            })
            .collect()
    };
    let mut out = Vec::new();
    for (t, t_off) in entries(base)? {
        if t & 0x8000_0000 != 0 || t_off & 0x8000_0000 == 0 {
            continue; // named types are not used here
        }
        for (id, id_off) in entries(base + (t_off & 0x7FFF_FFFF) as usize)? {
            if id & 0x8000_0000 != 0 || id_off & 0x8000_0000 == 0 {
                continue;
            }
            for (_, lang_off) in entries(base + (id_off & 0x7FFF_FFFF) as usize)? {
                let d = base + lang_off as usize;
                let rva = u32_at(d).ok_or_else(bad)?;
                let len = u32_at(d + 4).ok_or_else(bad)? as usize;
                let at = file(rva);
                let data = pe.get(at..at + len).ok_or_else(bad)?;
                out.push((t as u16, id as u16, data.to_vec()));
            }
        }
    }
    Ok(out)
}

/// The UTF-16 string value of `key` in an `RT_VERSION` resource (for tests).
pub fn version_string(resource: &[u8], key: &str) -> Option<String> {
    let needle: Vec<u8> = utf16z(key);
    let at = resource
        .windows(needle.len())
        .position(|w| w == needle.as_slice())?;
    let mut v = at + needle.len();
    v = v.next_multiple_of(4);
    let words: Vec<u16> = resource[v..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .take_while(|w| *w != 0)
        .collect();
    String::from_utf16(&words).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_ico() -> Vec<u8> {
        let png_a = b"\x89PNG-a-16".to_vec();
        let png_b = b"\x89PNG-b-32-longer".to_vec();
        let mut ico = vec![0, 0, 1, 0, 2, 0];
        let first = 6 + 32;
        for (w, data, off) in [(16u8, &png_a, first), (32, &png_b, first + png_a.len())] {
            ico.extend_from_slice(&[w, w, 0, 0, 1, 0, 32, 0]);
            ico.extend_from_slice(&(data.len() as u32).to_le_bytes());
            ico.extend_from_slice(&(off as u32).to_le_bytes());
        }
        ico.extend_from_slice(&png_a);
        ico.extend_from_slice(&png_b);
        ico
    }

    fn info() -> VersionInfo {
        VersionInfo {
            description: "Kim".into(),
            product: "Kim".into(),
            version: "0.0.1-preview.2".into(),
            file_name: "kim.exe".into(),
        }
    }

    #[test]
    fn ico_images_become_icon_resources_and_one_group() {
        let images = parse_ico(&tiny_ico()).unwrap();
        assert_eq!(images.len(), 2);
        assert_eq!(images[1].data, b"\x89PNG-b-32-longer");
        let g = group_icon(&images);
        assert_eq!(g.len(), 6 + 2 * 14);
        assert_eq!(&g[0..6], &[0, 0, 1, 0, 2, 0]);
        assert_eq!(g[6], 16);
        assert_eq!(
            u16::from_le_bytes([g[18], g[19]]),
            1,
            "the first image is RT_ICON 1"
        );
        assert_eq!(u16::from_le_bytes([g[32], g[33]]), 2);
        assert!(parse_ico(b"not an icon").is_err());
        assert!(parse_ico(&[0, 0, 1, 0, 0, 0]).is_err(), "no images");
    }

    #[test]
    fn version_numbers_drop_the_pre_release() {
        assert_eq!(version_numbers("1.2.3-preview.4"), [1, 2, 3, 0]);
        assert_eq!(version_numbers("0.0.1"), [0, 0, 1, 0]);
        assert_eq!(version_numbers("x"), [0, 0, 0, 0]);
    }

    #[test]
    fn version_resource_carries_the_names() {
        let v = version_resource(&info());
        assert_eq!(usize::from(u16::from_le_bytes([v[0], v[1]])), v.len());
        assert_eq!(
            version_string(&v, "FileDescription").as_deref(),
            Some("Kim")
        );
        assert_eq!(
            version_string(&v, "OriginalFilename").as_deref(),
            Some("kim.exe")
        );
        assert_eq!(
            version_string(&v, "ProductVersion").as_deref(),
            Some("0.0.1-preview.2")
        );
        assert!(v.windows(4).any(|w| w == 0xFEEF_04BD_u32.to_le_bytes()));
    }

    /// The object is a valid COFF file: one `.rsrc` section whose directory tree, followed by
    /// hand (applying the relocations as a linker would), leads to every resource.
    #[test]
    fn the_object_links_to_a_tree_that_reads_back() {
        let entries = program_resources(&tiny_ico(), &info()).unwrap();
        let o = object(Machine::X86_64, &entries);
        assert_eq!(u16::from_le_bytes([o[0], o[1]]), 0x8664);
        assert_eq!(&o[20..26], b".rsrc\0");
        let raw_len = u32::from_le_bytes(o[36..40].try_into().unwrap()) as usize;
        let raw = u32::from_le_bytes(o[40..44].try_into().unwrap()) as usize;
        let rel = u32::from_le_bytes(o[44..48].try_into().unwrap()) as usize;
        let nrel = usize::from(u16::from_le_bytes([o[52], o[53]]));
        assert_eq!(nrel, entries.len(), "one relocation per resource");
        // Build a fake PE around the section at RVA 0x1000 and apply the relocations.
        let va = 0x1000u32;
        let mut sect = o[raw..raw + raw_len].to_vec();
        for k in 0..nrel {
            let r = rel + 10 * k;
            let at = u32::from_le_bytes(o[r..r + 4].try_into().unwrap()) as usize;
            assert_eq!(u16::from_le_bytes([o[r + 8], o[r + 9]]), 3, "ADDR32NB");
            let v = u32::from_le_bytes(sect[at..at + 4].try_into().unwrap()) + va;
            sect[at..at + 4].copy_from_slice(&v.to_le_bytes());
        }
        let mut pe = vec![0u8; 0x200];
        pe[0x3C] = 0x40;
        pe[0x40..0x44].copy_from_slice(b"PE\0\0");
        let fh = 0x44;
        pe[fh + 2] = 1; // one section
        pe[fh + 16..fh + 18].copy_from_slice(&240u16.to_le_bytes());
        let opt = fh + 20;
        pe[opt..opt + 2].copy_from_slice(&0x20Bu16.to_le_bytes());
        pe[opt + 112 + 16..opt + 112 + 20].copy_from_slice(&va.to_le_bytes());
        let sh = opt + 240;
        pe[sh..sh + 5].copy_from_slice(b".rsrc");
        pe[sh + 8..sh + 12].copy_from_slice(&(sect.len() as u32).to_le_bytes());
        pe[sh + 12..sh + 16].copy_from_slice(&va.to_le_bytes());
        pe[sh + 16..sh + 20].copy_from_slice(&(sect.len() as u32).to_le_bytes());
        pe[sh + 20..sh + 24].copy_from_slice(&0x200u32.to_le_bytes());
        pe.extend_from_slice(&sect);
        let back = resources(&pe).unwrap();
        let mut want = entries;
        want.sort_by_key(|(t, id, _)| (*t, *id));
        assert_eq!(back, want);
    }
}
