//! Windows process spawning for the daedalus stub.
//!
//! Windows has no `fork`/`execvp`: the launcher spawns the app as a child
//! process via `CreateProcessW` and either waits for it (exit code passthrough)
//! or detaches (self-update re-exec). This module is compiled only for
//! `target_os = "windows"`.

#![cfg(target_os = "windows")]

use std::collections::BTreeMap;
use std::ffi::{c_void, OsStr, OsString};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

const CREATE_UNICODE_ENVIRONMENT: u32 = 0x0000_0400;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const DETACHED_PROCESS: u32 = 0x0000_0008;
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
const INFINITE: u32 = 0xFFFF_FFFF;
const WAIT_OBJECT_0: u32 = 0;
const WAIT_TIMEOUT: u32 = 0x0000_0102;
const STILL_ACTIVE: u32 = 259;

#[repr(C)]
struct StartupInfoW {
    cb: u32,
    lp_reserved: *mut u16,
    lp_desktop: *mut u16,
    lp_title: *mut u16,
    dw_x: u32,
    dw_y: u32,
    dw_x_size: u32,
    dw_y_size: u32,
    dw_x_count_chars: u32,
    dw_y_count_chars: u32,
    dw_fill_attribute: u32,
    dw_flags: u32,
    w_show_window: u16,
    cb_reserved2: u16,
    lp_reserved2: *mut u8,
    h_std_input: *mut c_void,
    h_std_output: *mut c_void,
    h_std_error: *mut c_void,
}

#[repr(C)]
struct ProcessInformation {
    h_process: *mut c_void,
    h_thread: *mut c_void,
    dw_process_id: u32,
    dw_thread_id: u32,
}

extern "system" {
    fn CreateProcessW(
        lp_application_name: *const u16,
        lp_command_line: *mut u16,
        lp_process_attributes: *mut c_void,
        lp_thread_attributes: *mut c_void,
        b_inherit_handles: i32,
        dw_creation_flags: u32,
        lp_environment: *mut c_void,
        lp_current_directory: *const u16,
        lp_startup_info: *mut StartupInfoW,
        lp_process_information: *mut ProcessInformation,
    ) -> i32;
    fn WaitForSingleObject(h_handle: *mut c_void, dw_milliseconds: u32) -> u32;
    fn GetExitCodeProcess(h_process: *mut c_void, lp_exit_code: *mut u32) -> i32;
    fn CloseHandle(h_object: *mut c_void) -> i32;
    fn GetLastError() -> u32;
}

/// A handle to a spawned child process.
pub struct Child {
    handle: *mut c_void,
    pub pid: u32,
}

impl Drop for Child {
    fn drop(&mut self) {
        // SAFETY: CloseHandle(2) on a valid process handle from
        // CreateProcessW. Closing the handle does not kill the process.
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

/// Spawn `prog` with `argv` and environment, optionally detached.
///
/// `argv[0]` is the program name. When `detached`, the child runs without a
/// console and the caller does not wait for it (used for self-update re-exec).
pub fn spawn(
    prog: &Path,
    argv: &[OsString],
    env: &BTreeMap<String, String>,
    cwd: Option<&Path>,
    detached: bool,
) -> io::Result<Child> {
    let command_line = build_command_line(argv);
    let env_block = build_env_block(env)?;

    let mut cmd_line_wide = to_wide(&command_line);
    // CreateProcessW may modify the command line buffer.
    cmd_line_wide.push(0);

    let mut si = StartupInfoW {
        cb: std::mem::size_of::<StartupInfoW>() as u32,
        lp_reserved: std::ptr::null_mut(),
        lp_desktop: std::ptr::null_mut(),
        lp_title: std::ptr::null_mut(),
        dw_x: 0,
        dw_y: 0,
        dw_x_size: 0,
        dw_y_size: 0,
        dw_x_count_chars: 0,
        dw_y_count_chars: 0,
        dw_fill_attribute: 0,
        dw_flags: 0,
        w_show_window: 0,
        cb_reserved2: 0,
        lp_reserved2: std::ptr::null_mut(),
        h_std_input: std::ptr::null_mut(),
        h_std_output: std::ptr::null_mut(),
        h_std_error: std::ptr::null_mut(),
    };
    let mut pi = ProcessInformation {
        h_process: std::ptr::null_mut(),
        h_thread: std::ptr::null_mut(),
        dw_process_id: 0,
        dw_thread_id: 0,
    };

    let cwd_wide = cwd.map(|c| to_wide_null(c.as_os_str()));
    let prog_wide = to_wide_null(prog.as_os_str());

    let mut flags = CREATE_UNICODE_ENVIRONMENT | CREATE_NEW_PROCESS_GROUP;
    if detached {
        flags |= CREATE_NO_WINDOW | DETACHED_PROCESS;
    }

    // SAFETY: all pointers are valid null-terminated UTF-16 buffers or
    // initialized structures. CreateProcessW copies the command line and
    // environment before returning.
    let ok = unsafe {
        CreateProcessW(
            prog_wide.as_ptr(),
            cmd_line_wide.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0, // bInheritHandles = FALSE
            flags,
            env_block.as_ptr() as *mut c_void,
            cwd_wide.as_ref().map_or(std::ptr::null(), Vec::as_ptr),
            &mut si,
            &mut pi,
        )
    };

    if ok == 0 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("CreateProcessW failed: {}", last_error()),
        ));
    }

    let pid = pi.dw_process_id;
    // SAFETY: pi.h_thread is a handle we own; close it (the process handle is
    // kept for waiting).
    unsafe {
        CloseHandle(pi.h_thread);
    }

    Ok(Child {
        handle: pi.h_process,
        pid,
    })
}

/// Block until the child exits and return its exit code.
pub fn wait(child: &Child) -> io::Result<i32> {
    // SAFETY: child.handle is a valid process handle from CreateProcessW.
    let rc = unsafe { WaitForSingleObject(child.handle, INFINITE) };
    if rc != WAIT_OBJECT_0 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("WaitForSingleObject failed: {rc}"),
        ));
    }
    exit_code(child)
}

/// Non-blocking poll: `Ok(None)` while running, `Ok(Some(code))` on exit.
pub fn try_wait(child: &Child) -> io::Result<Option<i32>> {
    // SAFETY: child.handle is a valid process handle; 0ms timeout polls.
    let rc = unsafe { WaitForSingleObject(child.handle, 0) };
    if rc == WAIT_OBJECT_0 {
        return exit_code(child).map(Some);
    }
    if rc == WAIT_TIMEOUT {
        return Ok(None);
    }
    Err(io::Error::new(
        io::ErrorKind::Other,
        format!("WaitForSingleObject failed: {rc}"),
    ))
}

fn exit_code(child: &Child) -> io::Result<i32> {
    let mut code: u32 = 0;
    // SAFETY: GetExitCodeProcess writes the exit code after success.
    let rc = unsafe { GetExitCodeProcess(child.handle, &mut code) };
    if rc == 0 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("GetExitCodeProcess failed: {}", last_error()),
        ));
    }
    // STILL_ACTIVE means the process is still running (should not happen
    // after a signaled wait); treat as 0 rather than a bogus code.
    Ok(if code == STILL_ACTIVE { 0 } else { code as i32 })
}

fn last_error() -> String {
    // SAFETY: GetLastError has no preconditions.
    format!("error {}", unsafe { GetLastError() })
}

/// Join argv into a Windows command line with quoting.
fn build_command_line(argv: &[OsString]) -> String {
    argv.iter()
        .map(|a| quote_arg(&a.to_string_lossy()))
        .collect::<Vec<String>>()
        .join(" ")
}

/// Quote a single command-line argument: wrap in quotes if it contains
/// spaces, tabs, or quotes (standard Windows argv parsing rules).
fn quote_arg(arg: &str) -> String {
    if arg.contains(' ') || arg.contains('\t') || arg.contains('"') {
        format!("\"{}\"", arg.replace('"', "\\\""))
    } else {
        arg.to_string()
    }
}

/// UTF-16LE environment block: `KEY=VALUE\0` pairs terminated by `\0`.
fn build_env_block(env: &BTreeMap<String, String>) -> io::Result<Vec<u16>> {
    let mut block = Vec::new();
    for (k, v) in env {
        for unit in format!("{k}={v}").encode_utf16() {
            block.push(unit);
        }
        block.push(0);
    }
    block.push(0);
    Ok(block)
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn to_wide_null(s: &OsStr) -> Vec<u16> {
    let mut v: Vec<u16> = s.encode_wide().collect();
    v.push(0);
    v
}
