//! Thin wrapper over a hidraw character device.
//!
//! Report buffers follow the hidraw userspace convention throughout: byte 0 is
//! the report ID (0 for unnumbered reports) and payload follows. uhid uses the
//! same convention on its side, so the proxy never has to add or strip IDs.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use anyhow::{Context, Result};

pub const HID_MAX_DESCRIPTOR_SIZE: usize = 4096;

const IOC_WRITE: u32 = 1;
const IOC_READ: u32 = 2;

const fn ioc(dir: u32, nr: u32, size: usize) -> libc::c_ulong {
    ((dir << 30) | ((size as u32) << 16) | ((b'H' as u32) << 8) | nr) as libc::c_ulong
}

#[repr(C)]
struct DevInfo {
    bustype: u32,
    vendor: i16,
    product: i16,
}

#[repr(C)]
struct ReportDescriptor {
    size: u32,
    value: [u8; HID_MAX_DESCRIPTOR_SIZE],
}

/// Matches `uhid_report_type` numbering so values pass straight through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportType {
    Feature = 0,
    Output = 1,
    Input = 2,
}

impl ReportType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Feature),
            1 => Some(Self::Output),
            2 => Some(Self::Input),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Info {
    pub bus: u16,
    pub vendor: u16,
    pub product: u16,
    pub name: String,
    pub phys: String,
    pub uniq: String,
    pub descriptor: Vec<u8>,
}

pub struct Hidraw {
    file: File,
}

impl Hidraw {
    pub fn open(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(path)
            .with_context(|| format!("open {}", path.display()))?;
        Ok(Self { file })
    }

    pub fn info(&self) -> Result<Info> {
        let fd = self.file.as_raw_fd();
        let mut di = DevInfo { bustype: 0, vendor: 0, product: 0 };
        // SAFETY: HIDIOCGRAWINFO fills a hidraw_devinfo of exactly this layout.
        check(unsafe { libc::ioctl(fd, ioc(IOC_READ, 0x03, size_of::<DevInfo>()), &mut di) })
            .context("HIDIOCGRAWINFO")?;

        let mut rd = ReportDescriptor { size: 0, value: [0; HID_MAX_DESCRIPTOR_SIZE] };
        // SAFETY: HIDIOCGRDESCSIZE writes an int; HIDIOCGRDESC fills the struct.
        check(unsafe { libc::ioctl(fd, ioc(IOC_READ, 0x01, size_of::<i32>()), &mut rd.size) })
            .context("HIDIOCGRDESCSIZE")?;
        check(unsafe { libc::ioctl(fd, ioc(IOC_READ, 0x02, size_of::<ReportDescriptor>()), &mut rd) })
            .context("HIDIOCGRDESC")?;

        Ok(Info {
            bus: di.bustype as u16,
            vendor: di.vendor as u16,
            product: di.product as u16,
            name: self.string_ioctl(0x04).context("HIDIOCGRAWNAME")?,
            phys: self.string_ioctl(0x05).context("HIDIOCGRAWPHYS")?,
            uniq: self.string_ioctl(0x08).context("HIDIOCGRAWUNIQ")?,
            descriptor: rd.value[..rd.size as usize].to_vec(),
        })
    }

    fn string_ioctl(&self, nr: u32) -> io::Result<String> {
        let mut buf = [0u8; 256];
        // SAFETY: the kernel writes at most `buf.len()` bytes and returns the count.
        let n = check(unsafe {
            libc::ioctl(self.file.as_raw_fd(), ioc(IOC_READ, nr, buf.len()), buf.as_mut_ptr())
        })? as usize;
        let s = &buf[..n.min(buf.len())];
        let s = s.split(|&b| b == 0).next().unwrap_or(&[]);
        Ok(String::from_utf8_lossy(s).into_owned())
    }

    pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        (&self.file).read(buf)
    }

    pub fn write(&self, data: &[u8]) -> io::Result<usize> {
        (&self.file).write(data)
    }

    /// GET_REPORT for `rnum`; returns the ID-prefixed report.
    pub fn get_report(&self, rtype: ReportType, rnum: u8) -> io::Result<Vec<u8>> {
        let nr = match rtype {
            ReportType::Feature => 0x07,
            ReportType::Output => 0x0C,
            ReportType::Input => 0x0A,
        };
        let mut buf = vec![0u8; HID_MAX_DESCRIPTOR_SIZE];
        buf[0] = rnum;
        // SAFETY: buffer length matches the size encoded in the ioctl number.
        let n = check(unsafe {
            libc::ioctl(self.file.as_raw_fd(), ioc(IOC_READ | IOC_WRITE, nr, buf.len()), buf.as_mut_ptr())
        })? as usize;
        buf.truncate(n.min(HID_MAX_DESCRIPTOR_SIZE));
        Ok(buf)
    }

    /// SET_REPORT with an ID-prefixed buffer.
    pub fn set_report(&self, rtype: ReportType, data: &[u8]) -> io::Result<usize> {
        let nr = match rtype {
            ReportType::Feature => 0x06,
            ReportType::Output => 0x0B,
            ReportType::Input => 0x09,
        };
        let mut buf = data.to_vec();
        // SAFETY: buffer length matches the size encoded in the ioctl number.
        let n = check(unsafe {
            libc::ioctl(self.file.as_raw_fd(), ioc(IOC_READ | IOC_WRITE, nr, buf.len()), buf.as_mut_ptr())
        })?;
        Ok(n as usize)
    }
}

impl AsRawFd for Hidraw {
    fn as_raw_fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }
}

fn check(ret: libc::c_int) -> io::Result<libc::c_int> {
    if ret < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(ret)
    }
}

/// Drops group/other bits and any POSIX ACL on a device node. Works for the
/// node's owner, which the udev rules make the daemon user. Guards against a
/// later udev RUN (such as the uaccess builtin) having re-granted access.
pub fn restrict_node(path: &Path) -> io::Result<()> {
    let c = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).map_err(io::Error::other)?;
    // SAFETY: c is a valid NUL-terminated path.
    check(unsafe { libc::chmod(c.as_ptr(), 0o600) })?;
    let key = c"system.posix_acl_access";
    // SAFETY: both pointers are valid C strings.
    let r = unsafe { libc::removexattr(c.as_ptr(), key.as_ptr()) };
    if r < 0 {
        let e = io::Error::last_os_error();
        if e.raw_os_error() != Some(libc::ENODATA) {
            return Err(e);
        }
    }
    Ok(())
}
