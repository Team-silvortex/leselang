//! Small, thread-affine safe wrapper around the app-owned AppKit bridge.
//! Native exceptions stop at the C boundary; Rust never owns an NSView pointer.

use crate::Error;
use leselang_vm::native::core::MAX_SCALAR_STRING_BYTES;
use std::{ffi::c_void, marker::PhantomData, ptr::NonNull, rc::Rc};

pub const WIDGET_COUNT: usize = 3;

#[repr(C)]
#[derive(Default)]
struct RawSnapshot {
    identity: usize,
    clicks: u64,
    mutation: u64,
    identifier_length: usize,
    title_length: usize,
    value_length: usize,
    role: i32,
    enabled: i32,
    editable: i32,
}

unsafe extern "C" {
    fn ll_gui_create(out: *mut *mut c_void) -> i32;
    fn ll_gui_destroy(gui: *mut c_void) -> i32;
    fn ll_gui_show(gui: *mut c_void) -> i32;
    fn ll_gui_close(gui: *mut c_void) -> i32;
    fn ll_gui_poll(gui: *mut c_void, dispatched: *mut i32) -> i32;
    fn ll_gui_is_open(gui: *mut c_void, open: *mut i32) -> i32;
    fn ll_gui_list(
        gui: *mut c_void,
        buffer: *mut u8,
        capacity: usize,
        length: *mut usize,
        count: *mut usize,
    ) -> i32;
    fn ll_gui_find(
        gui: *mut c_void,
        name: *const u8,
        len: usize,
        slot: *mut i32,
        identity: *mut usize,
    ) -> i32;
    fn ll_gui_snapshot(
        gui: *mut c_void,
        slot: i32,
        buffer: *mut u8,
        capacity: usize,
        out: *mut RawSnapshot,
    ) -> i32;
    fn ll_gui_click(gui: *mut c_void, slot: i32, expected_identity: usize) -> i32;
    fn ll_gui_set_text(
        gui: *mut c_void,
        slot: i32,
        expected_identity: usize,
        bytes: *const u8,
        len: usize,
    ) -> i32;
    fn ll_gui_set_editable(gui: *mut c_void, slot: i32, editable: i32) -> i32;
    fn ll_gui_replace(gui: *mut c_void, slot: i32) -> i32;
    fn ll_gui_set_title(gui: *mut c_void, slot: i32, bytes: *const u8, len: usize) -> i32;
    fn ll_gui_set_enabled(gui: *mut c_void, slot: i32, enabled: i32) -> i32;
    fn ll_gui_status(gui: *mut c_void, bytes: *const u8, len: usize) -> i32;
}

fn checked(code: i32) -> Result<(), Error> {
    match code {
        0 => Ok(()),
        1 => Err(Error::Gui(
            "AppKit rejected native work or raised an exception",
        )),
        2 => Err(Error::Gui("AppKit access requires the macOS main thread")),
        3 => Err(Error::Gui("AppKit window is closed")),
        4 => Err(Error::Gui("invalid AppKit slot or argument")),
        5 => Err(Error::Gui("AppKit text is not valid UTF-8")),
        6 => Err(Error::Gui("AppKit text exceeds host bounds")),
        7 => Err(Error::Gui("AppKit object was not found")),
        8 => Err(Error::Gui("AppKit object instance was replaced")),
        9 => Err(Error::Gui("AppKit control identifiers are ambiguous")),
        10 => Err(Error::Gui("AppKit control discovery exceeds tree bounds")),
        _ => Err(Error::Gui("unknown AppKit bridge error")),
    }
}
fn slot(value: usize) -> Result<i32, Error> {
    if value < WIDGET_COUNT {
        Ok(value as i32)
    } else {
        Err(Error::Gui("invalid AppKit slot"))
    }
}
fn bounded(value: &str) -> Result<(), Error> {
    if value.len() <= MAX_SCALAR_STRING_BYTES {
        Ok(())
    } else {
        Err(Error::Gui("AppKit text exceeds host bounds"))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WidgetSnapshot {
    pub identity: usize,
    pub identifier: String,
    pub role: String,
    pub title: String,
    pub value: String,
    pub editable: bool,
    pub enabled: bool,
    pub clicks: u64,
    /// Monotonic state serial catches changes even if visible values return to A.
    pub mutation: u64,
}

/// Owns the bridge context and all native views. The Rc marker forbids moving or
/// sharing it across threads; create also checks the actual macOS main thread.
///
/// ```compile_fail
/// use leselang_appkit_host::gui::Gui;
/// fn transfer(gui: Gui) { std::thread::spawn(move || drop(gui)); }
/// ```
pub struct Gui {
    raw: NonNull<c_void>,
    _main_thread: PhantomData<Rc<()>>,
}
impl Gui {
    pub fn new() -> Result<Self, Error> {
        let mut raw = std::ptr::null_mut();
        // SAFETY: valid out-pointer; success transfers one retained bridge context.
        checked(unsafe { ll_gui_create(&mut raw) })?;
        let raw = NonNull::new(raw).ok_or(Error::Gui("AppKit returned no window context"))?;
        Ok(Self {
            raw,
            _main_thread: PhantomData,
        })
    }
    pub fn show(&self) -> Result<(), Error> {
        // SAFETY: context is live and stays on its creating/main thread.
        checked(unsafe { ll_gui_show(self.raw.as_ptr()) })
    }
    pub fn close(&self) -> Result<(), Error> {
        // SAFETY: close leaves the retained context valid until Drop.
        checked(unsafe { ll_gui_close(self.raw.as_ptr()) })
    }
    pub fn is_open(&self) -> Result<bool, Error> {
        let mut open = 0;
        // SAFETY: live context and a valid output location.
        checked(unsafe { ll_gui_is_open(self.raw.as_ptr(), &mut open) })?;
        Ok(open != 0)
    }
    /// Exactly one nonblocking native event step; no VM polling or dispatch.
    pub fn pump_event(&self) -> Result<bool, Error> {
        let mut dispatched = 0;
        // SAFETY: live main-thread context and valid output location.
        checked(unsafe { ll_gui_poll(self.raw.as_ptr(), &mut dispatched) })?;
        Ok(dispatched != 0)
    }
    pub fn find(&self, name: &str) -> Result<usize, Error> {
        bounded(name)?;
        let mut found = -1;
        let mut identity = 0;
        // SAFETY: input bytes remain borrowed for this call; outputs are valid.
        checked(unsafe {
            ll_gui_find(
                self.raw.as_ptr(),
                name.as_ptr(),
                name.len(),
                &mut found,
                &mut identity,
            )
        })?;
        if found < 0 || found as usize >= WIDGET_COUNT || identity == 0 {
            return Err(Error::Gui("invalid AppKit discovery result"));
        }
        Ok(found as usize)
    }
    /// Bounded names from registered controls in the actual view tree. Names
    /// describe discovery only; find must still mint a live ObjectRef per session.
    pub fn list(&self) -> Result<Vec<String>, Error> {
        let mut bytes = [0u8; MAX_SCALAR_STRING_BYTES];
        let mut length = 0;
        let mut count = 0;
        // SAFETY: live main-thread context and bounded output buffer/locations.
        checked(unsafe {
            ll_gui_list(
                self.raw.as_ptr(),
                bytes.as_mut_ptr(),
                bytes.len(),
                &mut length,
                &mut count,
            )
        })?;
        if length > bytes.len() || count > WIDGET_COUNT || (count == 0) != (length == 0) {
            return Err(Error::Gui("invalid AppKit discovery list"));
        }
        if length == 0 {
            return Ok(Vec::new());
        }
        if bytes[length - 1] != 0 {
            return Err(Error::Gui("unterminated AppKit discovery list"));
        }
        let mut names = Vec::with_capacity(count);
        for item in bytes[..length - 1].split(|byte| *byte == 0) {
            let name = std::str::from_utf8(item)
                .map_err(|_| Error::Gui("invalid AppKit discovery UTF-8"))?;
            if name.is_empty() || names.iter().any(|existing| existing == name) {
                return Err(Error::Gui("ambiguous AppKit discovery list"));
            }
            names.push(name.to_owned());
        }
        if names.len() != count {
            return Err(Error::Gui("invalid AppKit discovery count"));
        }
        Ok(names)
    }
    pub fn snapshot(&self, index: usize) -> Result<WidgetSnapshot, Error> {
        let index = slot(index)?;
        let mut bytes = [0u8; MAX_SCALAR_STRING_BYTES * 3];
        let mut raw = RawSnapshot::default();
        // SAFETY: bridge writes at most capacity bytes and initializes repr(C) output.
        checked(unsafe {
            ll_gui_snapshot(
                self.raw.as_ptr(),
                index,
                bytes.as_mut_ptr(),
                bytes.len(),
                &mut raw,
            )
        })?;
        let lengths = [raw.identifier_length, raw.title_length, raw.value_length];
        if raw.identity == 0
            || lengths
                .iter()
                .any(|length| *length > MAX_SCALAR_STRING_BYTES)
            || !matches!(raw.enabled, 0 | 1)
            || !matches!(raw.editable, 0 | 1)
        {
            return Err(Error::Gui("invalid AppKit snapshot"));
        }
        let role = match raw.role {
            1 => "button",
            2 => "text_field",
            _ => return Err(Error::Gui("invalid AppKit control role")),
        };
        if raw.role == 1 && raw.editable != 0 {
            return Err(Error::Gui("button cannot be editable"));
        }
        // Each length was bounded before summing, so these offsets cannot overflow.
        let title_start = raw.identifier_length;
        let value_start = title_start + raw.title_length;
        let end = value_start + raw.value_length;
        let decode = |slice: &[u8]| {
            std::str::from_utf8(slice)
                .map(str::to_owned)
                .map_err(|_| Error::Gui("invalid AppKit UTF-8 snapshot"))
        };
        let identifier = decode(&bytes[..title_start])?;
        if identifier.is_empty() || identifier.contains('\0') {
            return Err(Error::Gui("invalid AppKit identifier"));
        }
        Ok(WidgetSnapshot {
            identity: raw.identity,
            identifier,
            role: role.into(),
            title: decode(&bytes[title_start..value_start])?,
            value: decode(&bytes[value_start..end])?,
            enabled: raw.enabled != 0,
            editable: raw.editable != 0,
            clicks: raw.clicks,
            mutation: raw.mutation,
        })
    }
    pub fn set_text(
        &self,
        index: usize,
        expected_identity: usize,
        text: &str,
    ) -> Result<(), Error> {
        let index = slot(index)?;
        bounded(text)?;
        // SAFETY: live context and input bytes; bridge validates actual role,
        // enabled/editable state and identity before mutation/notification.
        let code = unsafe {
            ll_gui_set_text(
                self.raw.as_ptr(),
                index,
                expected_identity,
                text.as_ptr(),
                text.len(),
            )
        };
        if code == 1 {
            return Err(Error::Uncertain(
                "AppKit text editing failed; native effects may remain",
            ));
        }
        checked(code)
    }
    /// Explicit app-owned external-change hook used by conformance checks.
    pub fn set_value(&self, index: usize, text: &str) -> Result<(), Error> {
        self.set_text(index, self.snapshot(index)?.identity, text)
    }
    pub fn set_editable(&self, index: usize, editable: bool) -> Result<(), Error> {
        // SAFETY: live main-thread context, validated slot and a C boolean value.
        checked(unsafe {
            ll_gui_set_editable(self.raw.as_ptr(), slot(index)?, i32::from(editable))
        })
    }
    pub fn click(&self, index: usize, expected_identity: usize) -> Result<(), Error> {
        let index = slot(index)?;
        // SAFETY: checked slot, live context; bridge checks identity before performClick.
        let code = unsafe { ll_gui_click(self.raw.as_ptr(), index, expected_identity) };
        if code == 1 {
            return Err(Error::Uncertain(
                "AppKit action raised an exception; native effects may remain",
            ));
        }
        checked(code)
    }
    pub fn replace(&self, index: usize) -> Result<(), Error> {
        // SAFETY: the bridge owns both native view instances and advances its token.
        checked(unsafe { ll_gui_replace(self.raw.as_ptr(), slot(index)?) })
    }
    pub fn set_title(&self, index: usize, title: &str) -> Result<(), Error> {
        bounded(title)?;
        // SAFETY: input bytes live for this synchronous call, slot is validated.
        checked(unsafe {
            ll_gui_set_title(self.raw.as_ptr(), slot(index)?, title.as_ptr(), title.len())
        })
    }
    pub fn set_enabled(&self, index: usize, enabled: bool) -> Result<(), Error> {
        // SAFETY: live context, validated slot and a C boolean value.
        checked(unsafe { ll_gui_set_enabled(self.raw.as_ptr(), slot(index)?, i32::from(enabled)) })
    }
    pub fn status(&self, text: &str) -> Result<(), Error> {
        bounded(text)?;
        // SAFETY: text bytes are borrowed for this synchronous call only.
        checked(unsafe { ll_gui_status(self.raw.as_ptr(), text.as_ptr(), text.len()) })
    }
}
impl Drop for Gui {
    fn drop(&mut self) {
        // SAFETY: exactly one destroy of the live retained context, on main thread.
        // Drop cannot propagate bridge errors; it never dispatches language work.
        let _ = unsafe { ll_gui_destroy(self.raw.as_ptr()) };
    }
}
