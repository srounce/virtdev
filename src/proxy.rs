//! Pumps reports between a uhid device and an optional source hidraw.
//!
//! The uhid device outlives the source. Requests arriving while no source is
//! attached are failed with EIO rather than queued, so consumers never hang on
//! a disconnected controller.

use std::io;
use std::os::fd::AsRawFd;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::thread;

use anyhow::{Context, Result};
use log::{debug, info, warn};

use crate::hidraw::{Hidraw, ReportType};
use crate::uhid::{Event, Uhid};

pub enum Ctrl {
    Attach(Hidraw),
    Detach,
}

pub struct Handle {
    tx: Sender<Ctrl>,
    wake: Arc<EventFd>,
}

impl Handle {
    pub fn send(&self, msg: Ctrl) {
        let _ = self.tx.send(msg);
        self.wake.signal();
    }
}

pub struct Proxy {
    name: String,
    uhid: Arc<Uhid>,
    source: Option<Arc<Hidraw>>,
    rx: Receiver<Ctrl>,
    wake: Arc<EventFd>,
}

impl Proxy {
    pub fn new(name: String, uhid: Uhid) -> Result<(Self, Handle)> {
        let (tx, rx) = std::sync::mpsc::channel();
        let wake = Arc::new(EventFd::new()?);
        let proxy = Self { name, uhid: Arc::new(uhid), source: None, rx, wake: wake.clone() };
        Ok((proxy, Handle { tx, wake }))
    }

    pub fn run(mut self) -> Result<()> {
        let mut buf = [0u8; crate::uhid::UHID_DATA_MAX];
        loop {
            let mut fds = vec![
                libc::pollfd { fd: self.uhid.as_raw_fd(), events: libc::POLLIN, revents: 0 },
                libc::pollfd { fd: self.wake.as_raw_fd(), events: libc::POLLIN, revents: 0 },
            ];
            if let Some(src) = &self.source {
                fds.push(libc::pollfd { fd: src.as_raw_fd(), events: libc::POLLIN, revents: 0 });
            }
            // SAFETY: fds is a valid array of pollfd for the duration of the call.
            let r = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as _, -1) };
            if r < 0 {
                let e = io::Error::last_os_error();
                if e.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(e).context("poll");
            }

            if fds[1].revents != 0 {
                self.wake.drain();
                while let Ok(msg) = self.rx.try_recv() {
                    match msg {
                        Ctrl::Attach(h) => {
                            info!("{}: source attached", self.name);
                            self.source = Some(Arc::new(h));
                        }
                        Ctrl::Detach => self.detach(),
                    }
                }
            }

            if fds.len() > 2 && fds[2].revents != 0 {
                self.pump_source(&mut buf);
            }

            if fds[0].revents != 0 {
                self.pump_uhid()?;
            }
        }
    }

    fn detach(&mut self) {
        if self.source.take().is_some() {
            info!("{}: source detached", self.name);
        }
    }

    fn pump_source(&mut self, buf: &mut [u8]) {
        let Some(src) = self.source.clone() else { return };
        loop {
            match src.read(buf) {
                Ok(0) => {
                    warn!("{}: source EOF", self.name);
                    self.detach();
                    return;
                }
                Ok(n) => {
                    debug!("{}: in  {}", self.name, hex(&buf[..n]));
                    if let Err(e) = self.uhid.input(&buf[..n]) {
                        warn!("{}: uhid input failed: {e}", self.name);
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    warn!("{}: source read failed: {e}", self.name);
                    self.detach();
                    return;
                }
            }
        }
    }

    fn pump_uhid(&mut self) -> Result<()> {
        loop {
            let ev = match self.uhid.read_event() {
                Ok(ev) => ev,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e).context("uhid read"),
            };
            match ev {
                Event::Start { dev_flags } => info!("{}: uhid start (flags {dev_flags:#x})", self.name),
                Event::Stop => info!("{}: uhid stop", self.name),
                Event::Open => info!("{}: uhid opened by a consumer", self.name),
                Event::Close => info!("{}: uhid closed by last consumer", self.name),
                Event::Output { rtype, data } => {
                    debug!("{}: out rtype={rtype} {}", self.name, hex(&data));
                    match &self.source {
                        Some(src) => {
                            if let Err(e) = src.write(&data) {
                                warn!("{}: source write failed: {e}", self.name);
                            }
                        }
                        None => debug!("{}: dropping output, no source", self.name),
                    }
                }
                Event::GetReport { id, rnum, rtype } => {
                    debug!("{}: get_report id={id} rnum={rnum:#04x} rtype={rtype}", self.name);
                    let (Some(src), Some(rt)) = (self.source.clone(), ReportType::from_u8(rtype)) else {
                        self.uhid.get_report_reply(id, libc::EIO as u16, &[])?;
                        continue;
                    };
                    let (uhid, name) = (self.uhid.clone(), self.name.clone());
                    thread::spawn(move || {
                        let res = match src.get_report(rt, rnum) {
                            Ok(data) => {
                                debug!("{name}: get_report id={id} -> {}", hex(&data));
                                uhid.get_report_reply(id, 0, &data)
                            }
                            Err(e) => {
                                warn!("{name}: get_report id={id} rnum={rnum:#04x} failed: {e}");
                                uhid.get_report_reply(id, e.raw_os_error().unwrap_or(libc::EIO) as u16, &[])
                            }
                        };
                        if let Err(e) = res {
                            warn!("{name}: get_report reply failed: {e}");
                        }
                    });
                }
                Event::SetReport { id, rnum, rtype, data } => {
                    debug!("{}: set_report id={id} rnum={rnum:#04x} rtype={rtype} {}", self.name, hex(&data));
                    let (Some(src), Some(rt)) = (self.source.clone(), ReportType::from_u8(rtype)) else {
                        self.uhid.set_report_reply(id, libc::EIO as u16)?;
                        continue;
                    };
                    let (uhid, name) = (self.uhid.clone(), self.name.clone());
                    thread::spawn(move || {
                        let err = match src.set_report(rt, &data) {
                            Ok(_) => 0,
                            Err(e) => {
                                warn!("{name}: set_report id={id} rnum={rnum:#04x} failed: {e}");
                                e.raw_os_error().unwrap_or(libc::EIO) as u16
                            }
                        };
                        if let Err(e) = uhid.set_report_reply(id, err) {
                            warn!("{name}: set_report reply failed: {e}");
                        }
                    });
                }
                Event::Unknown(t) => warn!("{}: unknown uhid event {t}", self.name),
            }
        }
    }
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ")
}

pub struct EventFd(libc::c_int);

impl EventFd {
    fn new() -> io::Result<Self> {
        // SAFETY: plain syscall with valid flags.
        let fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(fd))
    }
    fn signal(&self) {
        let one: u64 = 1;
        // SAFETY: writing 8 bytes from a valid u64.
        unsafe { libc::write(self.0, &one as *const u64 as *const _, 8) };
    }
    fn drain(&self) {
        let mut v: u64 = 0;
        // SAFETY: reading 8 bytes into a valid u64.
        unsafe { libc::read(self.0, &mut v as *mut u64 as *mut _, 8) };
    }
}

impl AsRawFd for EventFd {
    fn as_raw_fd(&self) -> i32 {
        self.0
    }
}

impl Drop for EventFd {
    fn drop(&mut self) {
        // SAFETY: fd is owned by this struct.
        unsafe { libc::close(self.0) };
    }
}
