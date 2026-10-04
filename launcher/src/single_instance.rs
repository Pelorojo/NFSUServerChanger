//! Only one launcher at a time: a second start brings the running one's window back
//! (restored if minimized, in front) and quits.
//!
//! Windows: a named mutex marks the running launcher, the second start finds its window
//! by title. Linux: an abstract Unix socket (gone automatically when the process ends,
//! even after a crash); the second start sends a message, the running launcher shows
//! its window itself.

/// Passed when the launcher restarts itself (after an update, as admin): the new process
/// then waits for the old one to quit instead of handing over to it.
pub const RESTARTED_ARG: &str = "--restarted";

/// How long a restarted launcher waits for the old process to quit.
const RESTART_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

pub fn restarted() -> bool {
    std::env::args_os().skip(1).any(|a| a == RESTARTED_ARG)
}

/// The claim of being the one running launcher, held until the process ends.
pub enum Claim {
    /// This is the only launcher (or the check wasn't possible; then it just runs).
    Primary(Guard),
    /// Another launcher runs and was brought to front; this one should quit.
    Secondary,
}

/// Tries to become the one running launcher; if another one runs, shows it instead.
pub fn claim() -> Claim {
    let deadline = std::time::Instant::now() + RESTART_WAIT;
    loop {
        match Guard::acquire() {
            Some(guard) => return Claim::Primary(guard),
            None if restarted() && std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            // After a restart the old process didn't quit in time: run anyway.
            None if restarted() => return Claim::Primary(Guard::none()),
            None => {
                return if show_running() {
                    Claim::Secondary
                } else {
                    // Couldn't reach it (e.g. still starting up): better two than none.
                    Claim::Primary(Guard::none())
                };
            }
        }
    }
}

// --- Windows -------------------------------------------------------------------------

#[cfg(windows)]
pub struct Guard {
    // Kept open for the whole run; the mutex exists as long as a handle to it is open.
    _mutex: Option<windows_sys::Win32::Foundation::HANDLE>,
}

#[cfg(windows)]
const WINDOW_TITLE: &str = "NFSU Server Changer";

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

#[cfg(windows)]
impl Guard {
    fn none() -> Self {
        Guard { _mutex: None }
    }

    fn acquire() -> Option<Self> {
        use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
        use windows_sys::Win32::System::Threading::CreateMutexW;
        let name = wide("Local\\NFSUServerChanger");
        // SAFETY: NUL-terminated name, default security, no initial ownership.
        let mutex = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if mutex.is_null() {
            // Access denied: an elevated launcher holds it.
            return None;
        }
        // SAFETY: plain thread-local error query right after the call.
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            // SAFETY: the handle was just returned by CreateMutexW.
            unsafe { CloseHandle(mutex) };
            return None;
        }
        Some(Guard {
            _mutex: Some(mutex),
        })
    }

    /// Nothing to serve on Windows: the second start restores the window itself. The mutex
    /// handle is never closed (Guard has no Drop), so the mutex lives until the process ends.
    pub fn serve(self, _show: impl Fn() + Send + 'static) {}
}

/// Restores and raises the running launcher's window. This process was just started by
/// the user, so it may put another window in front (Windows only allows the foreground app).
#[cfg(windows)]
fn show_running() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW,
    };
    let title = wide(WINDOW_TITLE);
    // The running launcher may still be creating its window: try for a moment.
    for _ in 0..25 {
        // SAFETY: NUL-terminated title, any window class.
        let hwnd = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
        if !hwnd.is_null() {
            // SAFETY: hwnd is a valid top-level window handle from FindWindowW.
            unsafe {
                ShowWindow(
                    hwnd,
                    if IsIconic(hwnd) != 0 {
                        SW_RESTORE
                    } else {
                        SW_SHOW
                    },
                );
                SetForegroundWindow(hwnd);
            }
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    false
}

// --- Linux ---------------------------------------------------------------------------

#[cfg(not(windows))]
pub struct Guard {
    listener: Option<std::os::unix::net::UnixListener>,
}

/// Abstract socket name, per user (all users share the namespace).
#[cfg(not(windows))]
fn socket_addr() -> std::io::Result<std::os::unix::net::SocketAddr> {
    use std::os::linux::net::SocketAddrExt;
    use std::os::unix::fs::MetadataExt;
    let uid = std::fs::metadata("/proc/self")
        .map(|m| m.uid())
        .unwrap_or(0);
    std::os::unix::net::SocketAddr::from_abstract_name(format!("nfsu-server-changer-{uid}"))
}

#[cfg(not(windows))]
impl Guard {
    fn none() -> Self {
        Guard { listener: None }
    }

    fn acquire() -> Option<Self> {
        let listener =
            socket_addr().and_then(|addr| std::os::unix::net::UnixListener::bind_addr(&addr));
        match listener {
            Ok(listener) => Some(Guard {
                listener: Some(listener),
            }),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => None,
            // No abstract sockets (not Linux): no single-instance check.
            Err(_) => Some(Guard::none()),
        }
    }

    /// Calls `show` (from a background thread) whenever another start asks for the window.
    pub fn serve(self, show: impl Fn() + Send + 'static) {
        let Some(listener) = self.listener else {
            return;
        };
        std::thread::spawn(move || {
            // The listener lives in this thread for the whole run, keeping the name taken.
            for _connection in listener.incoming().flatten() {
                show();
            }
        });
    }
}

#[cfg(not(windows))]
fn show_running() -> bool {
    socket_addr()
        .and_then(|addr| std::os::unix::net::UnixStream::connect_addr(&addr))
        .is_ok()
}
