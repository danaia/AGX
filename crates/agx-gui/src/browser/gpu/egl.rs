//! EGL DMA-BUF import and completion fences, used only with a current Slint GL context.
use super::transport::Frame;
use std::{
    ffi::{CStr, c_void},
    os::fd::AsRawFd,
    os::fd::{FromRawFd, OwnedFd},
    ptr,
};
type Handle = *mut c_void;
type CreateImage = unsafe extern "C" fn(Handle, Handle, u32, Handle, *const i32) -> Handle;
type Destroy = unsafe extern "C" fn(Handle, Handle) -> u32;
type CreateSync = unsafe extern "C" fn(Handle, u32, *const i32) -> Handle;
type Wait = unsafe extern "C" fn(Handle, Handle, i32, u64) -> i32;
type DupFence = unsafe extern "C" fn(Handle, Handle) -> i32;
type ImageTarget = unsafe extern "C" fn(u32, Handle);
pub struct Egl {
    _library: libloading::Library,
    display: Handle,
    create_image: CreateImage,
    destroy_image: Destroy,
    create_sync: CreateSync,
    destroy_sync: Destroy,
    wait: Wait,
    image_target: ImageTarget,
    dup_fence: Option<DupFence>,
}
impl Egl {
    pub unsafe fn load() -> Result<Self, String> {
        unsafe {
            let library = libloading::Library::new("libEGL.so.1").map_err(|e| e.to_string())?;
            let current = *library
                .get::<unsafe extern "C" fn() -> Handle>(b"eglGetCurrentDisplay\0")
                .map_err(|e| e.to_string())?;
            let display = current();
            if display.is_null() {
                return Err("No current EGL display".into());
            }
            let get = *library
                .get::<unsafe extern "C" fn(*const std::ffi::c_char) -> *const c_void>(
                    b"eglGetProcAddress\0",
                )
                .map_err(|e| e.to_string())?;
            unsafe fn function<T: Copy>(
                get: unsafe extern "C" fn(*const std::ffi::c_char) -> *const c_void,
                name: &CStr,
            ) -> Result<T, String> {
                let pointer = unsafe { get(name.as_ptr()) };
                if pointer.is_null() {
                    return Err(format!(
                        "Missing EGL entry point: {}",
                        name.to_string_lossy()
                    ));
                }
                Ok(unsafe { std::mem::transmute_copy(&pointer) })
            }
            let query = *library
                .get::<unsafe extern "C" fn(Handle, i32) -> *const std::ffi::c_char>(
                    b"eglQueryString\0",
                )
                .map_err(|e| e.to_string())?;
            let extensions = query(display, 0x3055);
            let native_sync = !extensions.is_null()
                && CStr::from_ptr(extensions)
                    .to_string_lossy()
                    .split_whitespace()
                    .any(|name| name == "EGL_ANDROID_native_fence_sync");
            let dup_fence = if native_sync {
                function(get, c"eglDupNativeFenceFDANDROID").ok()
            } else {
                None
            };
            Ok(Self {
                display,
                dup_fence,
                create_image: function(get, c"eglCreateImageKHR")?,
                destroy_image: function(get, c"eglDestroyImageKHR")?,
                create_sync: function(get, c"eglCreateSyncKHR")?,
                destroy_sync: function(get, c"eglDestroySyncKHR")?,
                wait: function(get, c"eglClientWaitSyncKHR")?,
                image_target: function(get, c"glEGLImageTargetTexture2DOES")?,
                _library: library,
            })
        }
    }
    pub unsafe fn import(&self, frame: &Frame) -> Result<Handle, String> {
        let mut attributes = vec![
            0x3057,
            frame.width as i32,
            0x3056,
            frame.height as i32,
            0x3271,
            frame.format as i32,
            0x3272,
            frame.fd.as_raw_fd(),
            0x3273,
            frame.offset as i32,
            0x3274,
            frame.stride as i32,
        ];
        // LINEAR needs no modifier extension. INVALID means the implicit driver layout.
        if frame.modifier != 0 && frame.modifier != u64::MAX {
            attributes.extend([
                0x3443,
                frame.modifier as i32,
                0x3444,
                (frame.modifier >> 32) as i32,
            ]);
        }
        attributes.push(0x3038);
        let image = unsafe {
            (self.create_image)(
                self.display,
                ptr::null_mut(),
                0x3270,
                ptr::null_mut(),
                attributes.as_ptr(),
            )
        };
        if image.is_null() {
            return Err("EGL DMA-BUF import failed".into());
        }
        Ok(image)
    }
    pub unsafe fn bind_image(&self, image: Handle) {
        unsafe { (self.image_target)(glow::TEXTURE_2D, image) }
    }
    pub unsafe fn fence(&self) -> Result<Handle, String> {
        let fence = unsafe {
            (self.create_sync)(
                self.display,
                if self.dup_fence.is_some() {
                    0x3144
                } else {
                    0x30F9
                },
                [0x3038].as_ptr(),
            )
        };
        if fence.is_null() {
            return Err("EGL fence creation failed".into());
        }
        Ok(fence)
    }
    // A bounded 1ms completion wait avoids delaying buffer reuse by a whole
    // extra display refresh when the GPU copy is just finishing. No readback.
    pub unsafe fn complete(&self, fence: Handle) -> Result<bool, String> {
        match unsafe { (self.wait)(self.display, fence, 0, 1_000_000) } {
            0x30F6 => Ok(true),
            0x30F5 => Ok(false),
            _ => Err("EGL fence wait failed".into()),
        }
    }
    pub unsafe fn export_fence(&self, fence: Handle) -> Option<OwnedFd> {
        let duplicate = self.dup_fence?;
        let fd = unsafe { duplicate(self.display, fence) };
        if fd < 0 {
            return None;
        }
        let owned = unsafe { OwnedFd::from_raw_fd(fd) };
        if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
            return None;
        }
        Some(owned)
    }
    pub unsafe fn destroy_image(&self, image: Handle) {
        unsafe {
            (self.destroy_image)(self.display, image);
        }
    }
    pub unsafe fn destroy_fence(&self, fence: Handle) {
        unsafe {
            (self.destroy_sync)(self.display, fence);
        }
    }
}
