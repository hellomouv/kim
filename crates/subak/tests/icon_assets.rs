//! Step 44 (`SUBAK-FR-054`, ADR 0073): Subak's icon assets are generated from the one renderer
//! the window, the floating icon and the tray draw with (`subak::gui::float::art`), and this
//! test proves the committed files are exactly what it produces, small, and readable at
//! 16–48 px on near-black and light surfaces.
//!
//! Regenerate (after changing the art): `SUBAK_WRITE_ICONS=1 cargo test -p subak --test icon_assets`.
#![cfg(windows)]

use std::path::{Path, PathBuf};
use subak::gui::float::art::{HostState, IconState, Palette, Picture, render};
use subak_term::render::Rgb;

const PNG_SIZES: [u32; 8] = [16, 20, 24, 32, 48, 64, 128, 256];
const ICO_SIZES: [u32; 9] = [16, 20, 24, 32, 40, 48, 64, 128, 256];
/// The surfaces the icon is checked on: near-black (a dark taskbar), black, white, light grey.
const SURFACES: [(&str, Rgb); 4] = [
    ("near-black", Rgb(0x0B, 0x0B, 0x0D)),
    ("black", Rgb(0, 0, 0)),
    ("white", Rgb(0xFF, 0xFF, 0xFF)),
    ("light", Rgb(0xF3, 0xF3, 0xF3)),
];

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/subak")
}

fn state() -> IconState {
    IconState {
        host: HostState::Connected,
        ..IconState::default()
    }
}

fn png(p: &Picture) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, p.size, p.size);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        e.set_compression(png::Compression::High);
        let mut w = e.write_header().unwrap();
        let mut data = Vec::with_capacity((p.size * p.size * 4) as usize);
        for y in 0..p.size {
            for x in 0..p.size {
                let (r, g, b, a) = p.rgba(x, y);
                data.extend_from_slice(&[r, g, b, a]);
            }
        }
        w.write_image_data(&data).unwrap();
    }
    out
}

/// A Windows Vista+ `.ico` with PNG entries.
fn ico(entries: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = vec![0, 0, 1, 0];
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * entries.len() as u32;
    for (size, data) in entries {
        let b = if *size >= 256 { 0 } else { *size as u8 };
        out.extend_from_slice(&[b, b, 0, 0, 1, 0, 32, 0]);
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in entries {
        out.extend_from_slice(data);
    }
    out
}

/// `p` over a surface colour, as straight RGB.
fn over(p: &Picture, x: u32, y: u32, bg: Rgb) -> (f32, f32, f32) {
    let (r, g, b, a) = p.rgba(x, y);
    let k = f32::from(a) / 255.0;
    let mix = |c: u8, s: u8| f32::from(c) * k + f32::from(s) * (1.0 - k);
    (mix(r, bg.0), mix(g, bg.1), mix(b, bg.2))
}

fn luminance((r, g, b): (f32, f32, f32)) -> f32 {
    let lin = |c: f32| {
        let c = c / 255.0;
        if c <= 0.039_28 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

fn contrast(a: (f32, f32, f32), b: (f32, f32, f32)) -> f32 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// A strip of the small sizes on a surface (the visual check, committed as a preview).
fn preview(bg: Rgb) -> Picture {
    let sizes = [16u32, 20, 24, 32, 48];
    let (w, h) = (sizes.iter().sum::<u32>() + 8 * (sizes.len() as u32 + 1), 64);
    // A square canvas wide enough; rows below the strip stay the surface.
    let size = w.max(h);
    let mut pixels =
        vec![
            0xFF00_0000 | (u32::from(bg.0) << 16) | (u32::from(bg.1) << 8) | u32::from(bg.2);
            (size * size) as usize
        ];
    let mut x0 = 8;
    for s in sizes {
        let p = render(&state(), s, &Palette::subak());
        let y0 = (h - s) / 2;
        for y in 0..s {
            for x in 0..s {
                let (r, g, b) = over(&p, x, y, bg);
                pixels[((y0 + y) * size + x0 + x) as usize] = 0xFF00_0000
                    | ((r.round() as u32) << 16)
                    | ((g.round() as u32) << 8)
                    | b.round() as u32;
            }
        }
        x0 += s + 8;
    }
    Picture { size, pixels }
}

fn expected() -> Vec<(String, Vec<u8>)> {
    let pal = Palette::subak();
    let mut files = Vec::new();
    for s in PNG_SIZES {
        files.push((format!("subak-{s}.png"), png(&render(&state(), s, &pal))));
    }
    // Monochrome / High Contrast variants: the outline in the text colour, both polarities.
    let black_on_white = Palette::high_contrast(Rgb(255, 255, 255), Rgb(0, 0, 0), Rgb(0, 0, 0xCC));
    let white_on_black =
        Palette::high_contrast(Rgb(0, 0, 0), Rgb(255, 255, 255), Rgb(0, 0xFF, 0xFF));
    for s in [16, 32, 48] {
        files.push((
            format!("subak-mono-dark-{s}.png"),
            png(&render(&state(), s, &black_on_white)),
        ));
        files.push((
            format!("subak-mono-light-{s}.png"),
            png(&render(&state(), s, &white_on_black)),
        ));
    }
    let entries: Vec<(u32, Vec<u8>)> = ICO_SIZES
        .iter()
        .map(|s| (*s, png(&render(&state(), *s, &pal))))
        .collect();
    files.push(("subak.ico".into(), ico(&entries)));
    files.push(("preview-dark.png".into(), png(&preview(SURFACES[0].1))));
    files.push(("preview-light.png".into(), png(&preview(SURFACES[3].1))));
    files
}

/// The decoded pixels of a PNG (the compressed bytes depend on the deflate backend that
/// feature unification picks, so files are compared by their pixels).
fn pixels(data: &[u8]) -> Option<(u32, Vec<u8>)> {
    let mut r = png::Decoder::new(std::io::Cursor::new(data))
        .read_info()
        .ok()?;
    let mut buf = vec![0; r.output_buffer_size()?];
    let info = r.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    Some((info.width, buf))
}

/// The PNG entries of an `.ico`.
fn ico_entries(data: &[u8]) -> Option<Vec<(u32, Vec<u8>)>> {
    let n = usize::from(u16::from_le_bytes([*data.get(4)?, *data.get(5)?]));
    (0..n)
        .map(|k| {
            let e = data.get(6 + 16 * k..6 + 16 * (k + 1))?;
            let len = u32::from_le_bytes(e[8..12].try_into().ok()?) as usize;
            let at = u32::from_le_bytes(e[12..16].try_into().ok()?) as usize;
            pixels(data.get(at..at + len)?)
        })
        .collect()
}

fn same(name: &str, have: &[u8], want: &[u8]) -> bool {
    if name.ends_with(".ico") {
        ico_entries(have).is_some_and(|h| Some(h) == ico_entries(want))
    } else {
        pixels(have).is_some_and(|h| Some(h) == pixels(want))
    }
}

#[test]
fn the_committed_icon_assets_are_exactly_what_the_renderer_produces() {
    let write = std::env::var_os("SUBAK_WRITE_ICONS").is_some();
    let mut stale = Vec::new();
    for (name, bytes) in expected() {
        let path = dir().join(&name);
        if write {
            std::fs::write(&path, &bytes).unwrap();
        } else if !std::fs::read(&path).is_ok_and(|have| same(&name, &have, &bytes)) {
            stale.push(name);
        }
    }
    assert!(
        stale.is_empty(),
        "stale icon assets {stale:?}: run with SUBAK_WRITE_ICONS=1"
    );
    let ico = std::fs::metadata(dir().join("subak.ico")).unwrap().len();
    assert!(ico < 64 * 1024, "subak.ico is {ico} bytes");
    for s in [16, 32, 48] {
        let n = std::fs::metadata(dir().join(format!("subak-{s}.png")))
            .unwrap()
            .len();
        assert!(n < 4 * 1024, "subak-{s}.png is {n} bytes");
    }
    println!(
        "EVIDENCE icon assets: subak.ico {ico} bytes ({} sizes)",
        ICO_SIZES.len()
    );
}

/// The vector master names the renderer's colours (one identity, two sources kept equal).
#[test]
fn the_svg_master_uses_the_renderers_colours() {
    let svg = std::fs::read_to_string(dir().join("subak.svg")).unwrap();
    let p = Palette::subak();
    for (what, c) in [
        ("tile", p.tile),
        ("edge", p.edge),
        ("rind", p.rind),
        ("rind edge", p.rind_edge),
        ("band", p.band),
        ("flesh", p.flesh),
        ("seed", p.seed),
    ] {
        let hex = format!("#{:02X}{:02X}{:02X}", c.0, c.1, c.2);
        assert!(svg.contains(&hex), "{what} {hex} missing from subak.svg");
    }
}

/// At 16–48 px the slice reads as black rind and red flesh, and the icon stands apart from
/// near-black and light surfaces (never black-on-black).
#[test]
fn the_icon_reads_at_small_sizes_on_dark_and_light_surfaces() {
    let pal = Palette::subak();
    let mut report = Vec::new();
    for s in [16u32, 20, 24, 32, 48] {
        let p = render(&state(), s, &pal);
        let opaque: Vec<(u32, u32)> = (0..s)
            .flat_map(|y| (0..s).map(move |x| (x, y)))
            .filter(|&(x, y)| p.rgba(x, y).3 == 255)
            .collect();
        let red = opaque
            .iter()
            .filter(|&&(x, y)| {
                let (r, g, b, _) = p.rgba(x, y);
                r > 180 && g < 90 && b < 90
            })
            .count();
        let charcoal = opaque
            .iter()
            .filter(|&&(x, y)| {
                let (r, g, b, _) = p.rgba(x, y);
                r.max(g).max(b) < 0x40
            })
            .count();
        let area = (s * s) as f32;
        assert!(red as f32 / area > 0.10, "{s} px: red flesh {red} px");
        assert!(
            charcoal >= (s as usize) / 2,
            "{s} px: charcoal rind {charcoal} px"
        );
        // The red flesh against the charcoal rind (the identity's own contrast).
        let flesh = (
            f32::from(pal.flesh.0),
            f32::from(pal.flesh.1),
            f32::from(pal.flesh.2),
        );
        let rind = (
            f32::from(pal.rind.0),
            f32::from(pal.rind.1),
            f32::from(pal.rind.2),
        );
        assert!(contrast(flesh, rind) >= 3.0);
        for (name, bg) in SURFACES {
            let b = (f32::from(bg.0), f32::from(bg.1), f32::from(bg.2));
            // Pixels that stand out from the surface by at least 3:1.
            let apart = (0..s)
                .flat_map(|y| (0..s).map(move |x| (x, y)))
                .filter(|&(x, y)| contrast(over(&p, x, y, bg), b) >= 3.0)
                .count();
            let share = apart as f32 / area;
            assert!(
                share >= 0.12,
                "{s} px on {name}: only {:.0}% stand apart",
                share * 100.0
            );
            report.push(format!("{s}px/{name} {:.0}%", share * 100.0));
        }
    }
    println!(
        "EVIDENCE icon legibility (pixels ≥ 3:1 against the surface): {}; flesh/rind contrast {:.2}:1",
        report.join(", "),
        contrast(
            (
                f32::from(pal.flesh.0),
                f32::from(pal.flesh.1),
                f32::from(pal.flesh.2)
            ),
            (
                f32::from(pal.rind.0),
                f32::from(pal.rind.1),
                f32::from(pal.rind.2)
            )
        )
    );
}

/// ADR 0089: `subak-gui.exe` and `subak.exe` carry the watermelon icon (`subak.ico`, linked by
/// kim-winres), so Explorer, the taskbar and the window's title bar show it.
#[test]
#[cfg(windows)]
fn the_programs_carry_the_watermelon_icon() {
    let images = kim_winres::parse_ico(&std::fs::read(dir().join("subak.ico")).unwrap()).unwrap();
    for exe in [env!("CARGO_BIN_EXE_subak-gui"), env!("CARGO_BIN_EXE_subak")] {
        let res = kim_winres::resources(&std::fs::read(exe).unwrap()).unwrap();
        let icons: Vec<&Vec<u8>> = res
            .iter()
            .filter(|r| r.0 == kim_winres::RT_ICON)
            .map(|r| &r.2)
            .collect();
        assert_eq!(icons.len(), images.len(), "{exe}");
        assert!(
            icons.iter().zip(&images).all(|(g, w)| **g == w.data),
            "{exe}"
        );
        let v = &res
            .iter()
            .find(|r| r.0 == kim_winres::RT_VERSION)
            .unwrap()
            .2;
        assert_eq!(
            kim_winres::version_string(v, "FileDescription").as_deref(),
            Some("Subak")
        );
    }
}
