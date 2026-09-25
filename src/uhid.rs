//! Wrapper over `/dev/uhid`. Events are serialised by hand against the packed
//! `struct uhid_event` layout from `linux/uhid.h`.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::OpenOptionsExt;

use anyhow::{Context, Result};

pub const UHID_DATA_MAX: usize = 4096;

/// sizeof(struct uhid_event): u32 type plus the largest union member (create2).
const EVENT_SIZE: usize = 4 + 128 + 64 + 64 + 2 + 2 + 4 + 4 + 4 + 4 + UHID_DATA_MAX;
const U: usize = 4; // union offset

const UHID_DESTROY: u32 = 1;
const UHID_START: u32 = 2;
const UHID_STOP: u32 = 3;
const UHID_OPEN: u32 = 4;
const UHID_CLOSE: u32 = 5;
const UHID_OUTPUT: u32 = 6;
const UHID_GET_REPORT: u32 = 9;
const UHID_GET_REPORT_REPLY: u32 = 10;
const UHID_CREATE2: u32 = 11;
const UHID_INPUT2: u32 = 12;
const UHID_SET_REPORT: u32 = 13;
const UHID_SET_REPORT_REPLY: u32 = 14;

#[derive(Clone, Debug)]
pub struct Identity {
    pub name: String,
    pub phys: String,
    pub uniq: String,
    pub bus: u16,
    pub vendor: u32,
    pub product: u32,
    pub version: u32,
    pub country: u32,
    pub descriptor: Vec<u8>,
}

#[derive(Debug)]
pub enum Event {
    Start { dev_flags: u64 },
    Stop,
    Open,
    Close,
    Output { rtype: u8, data: Vec<u8> },
    GetReport { id: u32, rnum: u8, rtype: u8 },
    SetReport { id: u32, rnum: u8, rtype: u8, data: Vec<u8> },
    Unknown(u32),
}

pub struct Uhid {
    file: File,
}

impl Uhid {
    pub fn create(ident: &Identity) -> Result<Self> {
        anyhow::ensure!(ident.descriptor.len() <= UHID_DATA_MAX, "report descriptor too large");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open("/dev/uhid")
            .context("open /dev/uhid")?;

        let mut ev = [0u8; EVENT_SIZE];
        put_u32(&mut ev, 0, UHID_CREATE2);
        put_str(&mut ev[U..U + 128], &ident.name);
        put_str(&mut ev[U + 128..U + 192], &ident.phys);
        put_str(&mut ev[U + 192..U + 256], &ident.uniq);
        put_u16(&mut ev, U + 256, ident.descriptor.len() as u16);
        put_u16(&mut ev, U + 258, ident.bus);
        put_u32(&mut ev, U + 260, ident.vendor);
        put_u32(&mut ev, U + 264, ident.product);
        put_u32(&mut ev, U + 268, ident.version);
        put_u32(&mut ev, U + 272, ident.country);
        ev[U + 276..U + 276 + ident.descriptor.len()].copy_from_slice(&ident.descriptor);
        (&file).write_all(&ev).context("UHID_CREATE2")?;
        Ok(Self { file })
    }

    pub fn input(&self, data: &[u8]) -> io::Result<()> {
        let mut ev = [0u8; EVENT_SIZE];
        let n = data.len().min(UHID_DATA_MAX);
        put_u32(&mut ev, 0, UHID_INPUT2);
        put_u16(&mut ev, U, n as u16);
        ev[U + 2..U + 2 + n].copy_from_slice(&data[..n]);
        (&self.file).write_all(&ev)
    }

    pub fn get_report_reply(&self, id: u32, err: u16, data: &[u8]) -> io::Result<()> {
        let mut ev = [0u8; EVENT_SIZE];
        let n = data.len().min(UHID_DATA_MAX);
        put_u32(&mut ev, 0, UHID_GET_REPORT_REPLY);
        put_u32(&mut ev, U, id);
        put_u16(&mut ev, U + 4, err);
        put_u16(&mut ev, U + 6, n as u16);
        ev[U + 8..U + 8 + n].copy_from_slice(&data[..n]);
        (&self.file).write_all(&ev)
    }

    pub fn set_report_reply(&self, id: u32, err: u16) -> io::Result<()> {
        let mut ev = [0u8; EVENT_SIZE];
        put_u32(&mut ev, 0, UHID_SET_REPORT_REPLY);
        put_u32(&mut ev, U, id);
        put_u16(&mut ev, U + 4, err);
        (&self.file).write_all(&ev)
    }

    pub fn destroy(&self) -> io::Result<()> {
        let mut ev = [0u8; EVENT_SIZE];
        put_u32(&mut ev, 0, UHID_DESTROY);
        (&self.file).write_all(&ev)
    }

    /// Reads one event. Returns `WouldBlock` when nothing is pending.
    pub fn read_event(&self) -> io::Result<Event> {
        let mut ev = [0u8; EVENT_SIZE];
        let n = (&self.file).read(&mut ev)?;
        if n < 4 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "short uhid event"));
        }
        let ty = get_u32(&ev, 0);
        Ok(match ty {
            UHID_START => Event::Start { dev_flags: u64::from_ne_bytes(ev[U..U + 8].try_into().unwrap()) },
            UHID_STOP => Event::Stop,
            UHID_OPEN => Event::Open,
            UHID_CLOSE => Event::Close,
            UHID_OUTPUT => {
                let size = get_u16(&ev, U + UHID_DATA_MAX) as usize;
                let rtype = ev[U + UHID_DATA_MAX + 2];
                Event::Output { rtype, data: ev[U..U + size.min(UHID_DATA_MAX)].to_vec() }
            }
            UHID_GET_REPORT => Event::GetReport { id: get_u32(&ev, U), rnum: ev[U + 4], rtype: ev[U + 5] },
            UHID_SET_REPORT => {
                let size = get_u16(&ev, U + 6) as usize;
                Event::SetReport {
                    id: get_u32(&ev, U),
                    rnum: ev[U + 4],
                    rtype: ev[U + 5],
                    data: ev[U + 8..U + 8 + size.min(UHID_DATA_MAX)].to_vec(),
                }
            }
            other => Event::Unknown(other),
        })
    }
}

impl AsRawFd for Uhid {
    fn as_raw_fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }
}

impl Drop for Uhid {
    fn drop(&mut self) {
        let _ = self.destroy();
    }
}

fn put_str(dst: &mut [u8], s: &str) {
    let b = s.as_bytes();
    let n = b.len().min(dst.len() - 1);
    dst[..n].copy_from_slice(&b[..n]);
}
fn put_u16(b: &mut [u8], at: usize, v: u16) {
    b[at..at + 2].copy_from_slice(&v.to_ne_bytes());
}
fn put_u32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_ne_bytes());
}
fn get_u16(b: &[u8], at: usize) -> u16 {
    u16::from_ne_bytes(b[at..at + 2].try_into().unwrap())
}
fn get_u32(b: &[u8], at: usize) -> u32 {
    u32::from_ne_bytes(b[at..at + 4].try_into().unwrap())
}
