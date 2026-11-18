//! Versioned payload for one system mutation.
//!
//! `previous` is the value to restore. `new` is what apply installs.
//! Reverting in reverse order therefore returns the process to the
//! pre-journal state even when several mutations touch the same object.

/// Windows priority class, independent of the Win32 constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PriorityClass {
    Idle,
    BelowNormal,
    Normal,
    AboveNormal,
    High,
    Realtime,
}

impl PriorityClass {
    /// Higher rank is a more favorable scheduling class.
    pub fn rank(self) -> u8 {
        match self {
            Self::Idle => 0,
            Self::BelowNormal => 1,
            Self::Normal => 2,
            Self::AboveNormal => 3,
            Self::High => 4,
            Self::Realtime => 5,
        }
    }

    fn tag(self) -> u8 {
        match self {
            Self::Idle => 1,
            Self::BelowNormal => 2,
            Self::Normal => 3,
            Self::AboveNormal => 4,
            Self::High => 5,
            Self::Realtime => 6,
        }
    }

    fn from_tag(tag: u8) -> Option<Self> {
        Some(match tag {
            1 => Self::Idle,
            2 => Self::BelowNormal,
            3 => Self::Normal,
            4 => Self::AboveNormal,
            5 => Self::High,
            6 => Self::Realtime,
            _ => return None,
        })
    }
}

/// Integer window rectangle in pixels. Origin is the top-left of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// One reversible change to the user's Windows session.
///
/// Record this value and `fsync` it *before* applying it. A crash between
/// the two steps is safe: revert installs `previous`, which is still the
/// live value when apply never ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mutation {
    /// `SetPriorityClass` for `pid`.
    ProcessPriority {
        pid: u32,
        previous: PriorityClass,
        new: PriorityClass,
    },
    /// Job-object CPU cap. `new_percent == 0` clears the cap.
    /// `previous_percent == None` means the process was uncapped.
    JobCap {
        pid: u32,
        previous_percent: Option<u32>,
        new_percent: u32,
    },
    /// `NtSuspendProcess`. Revert calls `NtResumeProcess`.
    /// Only recorded when the operator explicitly opts in.
    Suspend { pid: u32 },
    /// Native taskbar hidden via `ShowWindow(SW_HIDE)`.
    TaskbarHide,
    /// `SHAppBarMessage(ABM_NEW / ABM_SETPOS)`. Revert is `ABM_REMOVE`.
    AppBar { hwnd: u64, edge: u32, rect: Rect },
    /// `NtSetTimerResolution`. `previous_100ns == 0` means revert clears
    /// this process's request (`SetResolution = FALSE`) rather than
    /// installing a specific value.
    TimerResolution { previous_100ns: u32, new_100ns: u32 },
    /// `DWMWA_CLOAK` set on a foreign window.
    WindowCloak { hwnd: u64 },
    /// `ShowWindow(SW_HIDE)` on a foreign window.
    WindowHide { hwnd: u64 },
    /// `SetWindowPos` on a foreign window. `previous` is the rect Nexus
    /// observed before it moved the window.
    WindowPosition {
        hwnd: u64,
        previous: Rect,
        new: Rect,
    },
    /// `SetProcessInformation(ProcessPowerThrottling)`. `enabled` is what
    /// apply installs. Revert installs `previous_enabled`.
    EcoQos {
        pid: u32,
        previous_enabled: bool,
        enabled: bool,
    },
}

const KIND_PRIORITY: u8 = 1;
const KIND_JOB: u8 = 2;
const KIND_SUSPEND: u8 = 3;
const KIND_TASKBAR: u8 = 4;
const KIND_APPBAR: u8 = 5;
const KIND_TIMER: u8 = 6;
const KIND_CLOAK: u8 = 7;
const KIND_HIDE: u8 = 8;
const KIND_POSITION: u8 = 9;
const KIND_ECOQOS: u8 = 10;

struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    fn new() -> Self {
        Self { buf: Vec::new() }
    }

    fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn i32(&mut self, v: i32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn rect(&mut self, r: &Rect) {
        self.i32(r.x);
        self.i32(r.y);
        self.i32(r.width);
        self.i32(r.height);
    }
}

struct Reader<'a> {
    data: &'a [u8],
    i: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, i: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.i)
    }

    fn u8(&mut self) -> Option<u8> {
        let b = *self.data.get(self.i)?;
        self.i += 1;
        Some(b)
    }

    fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.remaining() < n {
            return None;
        }
        let start = self.i;
        self.i += n;
        Some(&self.data[start..self.i])
    }

    fn u32(&mut self) -> Option<u32> {
        let b = self.bytes(4)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u64(&mut self) -> Option<u64> {
        let b = self.bytes(8)?;
        Some(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    fn i32(&mut self) -> Option<i32> {
        self.u32().map(|v| v as i32)
    }

    fn rect(&mut self) -> Option<Rect> {
        Some(Rect {
            x: self.i32()?,
            y: self.i32()?,
            width: self.i32()?,
            height: self.i32()?,
        })
    }
}

pub fn encode_payload(mutation: &Mutation) -> Vec<u8> {
    let mut w = Writer::new();
    match mutation {
        Mutation::ProcessPriority { pid, previous, new } => {
            w.u8(KIND_PRIORITY);
            w.u32(*pid);
            w.u8(previous.tag());
            w.u8(new.tag());
        }
        Mutation::JobCap {
            pid,
            previous_percent,
            new_percent,
        } => {
            w.u8(KIND_JOB);
            w.u32(*pid);
            match previous_percent {
                Some(p) => {
                    w.u8(1);
                    w.u32(*p);
                }
                None => {
                    w.u8(0);
                    w.u32(0);
                }
            }
            w.u32(*new_percent);
        }
        Mutation::Suspend { pid } => {
            w.u8(KIND_SUSPEND);
            w.u32(*pid);
        }
        Mutation::TaskbarHide => {
            w.u8(KIND_TASKBAR);
        }
        Mutation::AppBar { hwnd, edge, rect } => {
            w.u8(KIND_APPBAR);
            w.u64(*hwnd);
            w.u32(*edge);
            w.rect(rect);
        }
        Mutation::TimerResolution {
            previous_100ns,
            new_100ns,
        } => {
            w.u8(KIND_TIMER);
            w.u32(*previous_100ns);
            w.u32(*new_100ns);
        }
        Mutation::WindowCloak { hwnd } => {
            w.u8(KIND_CLOAK);
            w.u64(*hwnd);
        }
        Mutation::WindowHide { hwnd } => {
            w.u8(KIND_HIDE);
            w.u64(*hwnd);
        }
        Mutation::WindowPosition {
            hwnd,
            previous,
            new,
        } => {
            w.u8(KIND_POSITION);
            w.u64(*hwnd);
            w.rect(previous);
            w.rect(new);
        }
        Mutation::EcoQos {
            pid,
            previous_enabled,
            enabled,
        } => {
            w.u8(KIND_ECOQOS);
            w.u32(*pid);
            w.u8(u8::from(*previous_enabled));
            w.u8(u8::from(*enabled));
        }
    }
    w.buf
}

pub fn decode_payload(bytes: &[u8]) -> Option<Mutation> {
    let mut r = Reader::new(bytes);
    let kind = r.u8()?;
    let mutation = match kind {
        KIND_PRIORITY => Mutation::ProcessPriority {
            pid: r.u32()?,
            previous: PriorityClass::from_tag(r.u8()?)?,
            new: PriorityClass::from_tag(r.u8()?)?,
        },
        KIND_JOB => {
            let pid = r.u32()?;
            let has_prev = r.u8()?;
            let prev_raw = r.u32()?;
            let new_percent = r.u32()?;
            let previous_percent = if has_prev == 1 { Some(prev_raw) } else { None };
            Mutation::JobCap {
                pid,
                previous_percent,
                new_percent,
            }
        }
        KIND_SUSPEND => Mutation::Suspend { pid: r.u32()? },
        KIND_TASKBAR => Mutation::TaskbarHide,
        KIND_APPBAR => Mutation::AppBar {
            hwnd: r.u64()?,
            edge: r.u32()?,
            rect: r.rect()?,
        },
        KIND_TIMER => Mutation::TimerResolution {
            previous_100ns: r.u32()?,
            new_100ns: r.u32()?,
        },
        KIND_CLOAK => Mutation::WindowCloak { hwnd: r.u64()? },
        KIND_HIDE => Mutation::WindowHide { hwnd: r.u64()? },
        KIND_POSITION => Mutation::WindowPosition {
            hwnd: r.u64()?,
            previous: r.rect()?,
            new: r.rect()?,
        },
        KIND_ECOQOS => Mutation::EcoQos {
            pid: r.u32()?,
            previous_enabled: r.u8()? == 1,
            enabled: r.u8()? == 1,
        },
        _ => return None,
    };
    if r.remaining() != 0 {
        return None;
    }
    Some(mutation)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples() -> Vec<Mutation> {
        vec![
            Mutation::ProcessPriority {
                pid: 42,
                previous: PriorityClass::Normal,
                new: PriorityClass::AboveNormal,
            },
            Mutation::JobCap {
                pid: 7,
                previous_percent: None,
                new_percent: 1,
            },
            Mutation::JobCap {
                pid: 7,
                previous_percent: Some(1),
                new_percent: 0,
            },
            Mutation::Suspend { pid: 9 },
            Mutation::TaskbarHide,
            Mutation::AppBar {
                hwnd: 0x1_0000,
                edge: 1,
                rect: Rect {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 28,
                },
            },
            Mutation::TimerResolution {
                previous_100ns: 156_250,
                new_100ns: 5_000,
            },
            Mutation::WindowCloak { hwnd: 99 },
            Mutation::WindowHide { hwnd: 100 },
            Mutation::WindowPosition {
                hwnd: 5,
                previous: Rect {
                    x: 10,
                    y: 20,
                    width: 800,
                    height: 600,
                },
                new: Rect {
                    x: 12,
                    y: 40,
                    width: 900,
                    height: 700,
                },
            },
            Mutation::EcoQos {
                pid: 11,
                previous_enabled: false,
                enabled: true,
            },
        ]
    }

    #[test]
    fn payload_roundtrip_every_variant() {
        for mutation in samples() {
            let bytes = encode_payload(&mutation);
            assert_eq!(decode_payload(&bytes), Some(mutation));
        }
    }

    #[test]
    fn truncated_payload_is_none() {
        let bytes = encode_payload(&Mutation::Suspend { pid: 1 });
        assert!(decode_payload(&bytes[..bytes.len() - 1]).is_none());
    }
}
