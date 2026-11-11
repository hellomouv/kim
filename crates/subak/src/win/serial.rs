//! Step 36 (ADR 0069): serial ports — discovery from `HKLM\HARDWARE\DEVICEMAP\SERIALCOMM` (read
//! only) and a port opened with the line settings the user chose. Reads wait at most 50 ms, so
//! the bridge never spins and never blocks writes for long; nothing runs in the Host.

use super::{Handle, wide};
use std::io;
use windows_sys::Win32::Devices::Communication::{
    COMMTIMEOUTS, DCB, GetCommState, SetCommState, SetCommTimeouts,
};
use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, OPEN_EXISTING, ReadFile, WriteFile,
};

/// The serial ports Windows lists: (device, driver name), e.g. (`COM3`, `\Device\Serial0`).
pub fn ports() -> Vec<(String, String)> {
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RegCloseKey, RegEnumValueW, RegOpenKeyExW,
    };
    let base = wide(r"HARDWARE\DEVICEMAP\SERIALCOMM");
    let mut key: HKEY = std::ptr::null_mut();
    // SAFETY: valid NUL-terminated name; `key` receives an opened handle on success.
    if unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, base.as_ptr(), 0, KEY_READ, &raw mut key) } != 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for i in 0..256u32 {
        let mut name = [0u16; 256];
        let mut nlen = name.len() as u32;
        let mut data = [0u16; 64];
        let mut dlen = (data.len() * 2) as u32;
        let mut ty = 0u32;
        // SAFETY: the buffers hold the sizes given.
        let r = unsafe {
            RegEnumValueW(
                key,
                i,
                name.as_mut_ptr(),
                &raw mut nlen,
                std::ptr::null(),
                &raw mut ty,
                data.as_mut_ptr().cast(),
                &raw mut dlen,
            )
        };
        if r != 0 {
            break;
        }
        let n = (dlen as usize / 2).min(data.len());
        let dev = String::from_utf16_lossy(&data[..n])
            .trim_end_matches('\0')
            .to_string();
        let driver = String::from_utf16_lossy(&name[..nlen as usize]);
        if valid_port(&dev) {
            out.push((dev, driver));
        }
    }
    // SAFETY: opened above.
    unsafe { RegCloseKey(key) };
    out.sort_by_key(|(d, _)| d[3..].parse::<u32>().unwrap_or(u32::MAX));
    out
}

/// `COM1`…`COM256`.
pub fn valid_port(p: &str) -> bool {
    p.len() > 3
        && p[..3].eq_ignore_ascii_case("COM")
        && p[3..].parse::<u32>().is_ok_and(|n| (1..=256).contains(&n))
}

/// Line settings (`DCB`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Line {
    pub baud: u32,
    pub data_bits: u8,
    /// 0 none, 1 odd, 2 even, 3 mark, 4 space (`DCB.Parity`).
    pub parity: u8,
    /// 0 one, 1 one and a half, 2 two (`DCB.StopBits`).
    pub stop_bits: u8,
    pub rtscts: bool,
    pub xonxoff: bool,
}

/// The `DCB` bit field for `l`: binary mode, DTR on, RTS on or handshake, XON/XOFF both ways.
pub fn dcb_bits(l: &Line) -> u32 {
    let mut b = 1u32; // fBinary
    if l.parity != 0 {
        b |= 1 << 1; // fParity
    }
    if l.rtscts {
        b |= 1 << 2; // fOutxCtsFlow
    }
    b |= 1 << 4; // fDtrControl = DTR_CONTROL_ENABLE
    if l.xonxoff {
        b |= (1 << 8) | (1 << 9); // fOutX, fInX
    }
    b |= if l.rtscts { 2 << 12 } else { 1 << 12 }; // fRtsControl: HANDSHAKE or ENABLE
    b
}

/// An open port.
pub struct Port(Handle);

impl Port {
    pub fn open(device: &str, l: &Line) -> io::Result<Self> {
        if !valid_port(device) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not COM1…COM256",
            ));
        }
        let path = wide(format!(r"\\.\{device}"));
        // SAFETY: a NUL-terminated device path; exclusive access; the result is checked.
        let h = Handle::new(unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            )
        })?;
        // SAFETY: a plain C struct; all-zero is valid.
        let mut dcb: DCB = unsafe { std::mem::zeroed() };
        dcb.DCBlength = u32::try_from(size_of::<DCB>()).unwrap_or(0);
        // SAFETY: `h` is the open port; `dcb` is sized.
        if unsafe { GetCommState(h.raw(), &raw mut dcb) } == 0 {
            return Err(io::Error::last_os_error());
        }
        dcb.BaudRate = l.baud;
        dcb._bitfield = dcb_bits(l);
        dcb.ByteSize = l.data_bits;
        dcb.Parity = l.parity;
        dcb.StopBits = l.stop_bits;
        dcb.XonChar = 0x11;
        dcb.XoffChar = 0x13;
        dcb.XonLim = 2048;
        dcb.XoffLim = 512;
        // SAFETY: as above.
        if unsafe { SetCommState(h.raw(), &raw const dcb) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // A read returns what arrived, or nothing after 50 ms; writes may take a second.
        let t = COMMTIMEOUTS {
            ReadIntervalTimeout: u32::MAX,
            ReadTotalTimeoutMultiplier: u32::MAX,
            ReadTotalTimeoutConstant: 50,
            WriteTotalTimeoutMultiplier: 0,
            WriteTotalTimeoutConstant: 1000,
        };
        // SAFETY: as above.
        if unsafe { SetCommTimeouts(h.raw(), &raw const t) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(h))
    }

    /// Reads what arrived within 50 ms (`Ok(0)`: nothing yet).
    pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        let mut n = 0u32;
        let len = u32::try_from(buf.len()).unwrap_or(u32::MAX);
        // SAFETY: `buf` holds `len` bytes; a synchronous handle.
        if unsafe {
            ReadFile(
                self.0.raw(),
                buf.as_mut_ptr(),
                len,
                &raw mut n,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(n as usize)
    }

    pub fn write_all(&self, mut buf: &[u8]) -> io::Result<()> {
        while !buf.is_empty() {
            let mut n = 0u32;
            let len = u32::try_from(buf.len()).unwrap_or(u32::MAX);
            // SAFETY: `buf` holds `len` bytes.
            if unsafe {
                WriteFile(
                    self.0.raw(),
                    buf.as_ptr(),
                    len,
                    &raw mut n,
                    std::ptr::null_mut(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "the port takes no data",
                ));
            }
            buf = &buf[n as usize..];
        }
        Ok(())
    }
}
