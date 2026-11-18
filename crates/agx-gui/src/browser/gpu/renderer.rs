//! GPU-to-GPU copy into a reusable Slint texture. No pixel readback or CPU upload.
//! Retaining the imported source until its EGL fence signals makes WebKit reuse safe.
use super::{completion, egl::Egl, transport::Frame};
use glow::HasContext;
use slint::{BorrowedOpenGLTextureBuilder, GraphicsAPI, Image};
use std::{
    ffi::{CString, c_void},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
    },
    time::Instant,
};
type Commands = SyncSender<(u8, Vec<u8>)>;
struct Source {
    frame: Frame,
    image: *mut c_void,
    texture: glow::NativeTexture,
    fbo: glow::NativeFramebuffer,
    fence: *mut c_void,
    commands: Commands,
    ready: Arc<AtomicBool>,
    async_ack: bool,
    counted: bool,
}
pub struct Renderer {
    gl: glow::Context,
    completion: completion::Worker,
    egl: Egl,
    outputs: [Option<glow::NativeTexture>; 2],
    output_index: usize,
    size: (u32, u32),
    source: Option<Source>,
    trace: bool,
    since: Instant,
    frames: u32,
    submit_us: u128,
}
impl Renderer {
    pub unsafe fn new(api: &GraphicsAPI) -> Result<Self, String> {
        let GraphicsAPI::NativeOpenGL { get_proc_address } = api else {
            return Err("No OpenGL renderer".into());
        };
        let gl = unsafe {
            glow::Context::from_loader_function(|name| {
                get_proc_address(&CString::new(name).unwrap())
            })
        };
        let egl = unsafe { Egl::load()? };
        // Probe synchronization before opting the host into GPU-only frame delivery.
        let fence = unsafe { egl.fence()? };
        unsafe {
            gl.flush();
            gl.finish();
            egl.destroy_fence(fence);
        }
        Ok(Self {
            gl,
            completion: completion::Worker::new().map_err(|e| e.to_string())?,
            egl,
            outputs: [None, None],
            output_index: 0,
            size: (0, 0),
            source: None,
            trace: std::env::var_os("AGX_WEB_TRACE").is_some(),
            since: Instant::now(),
            frames: 0,
            submit_us: 0,
        })
    }
    pub fn has_buffers(&self) -> bool {
        self.source.is_some() || self.outputs.iter().any(Option::is_some)
    }
    pub fn pending(&self) -> bool {
        self.source.is_some()
    }
    pub unsafe fn copy(
        &mut self,
        frame: Frame,
        commands: Commands,
    ) -> Result<Image, String> {
        let started = Instant::now();
        let sequence = frame.sequence;
        if std::env::var("AGX_WEB_GPU").as_deref() == Ok("probe-fallback") {
            let _ = commands.try_send((16, sequence.to_be_bytes().to_vec()));
            return Err("Requested GPU-to-CPU fallback probe".into());
        }
        let result = unsafe { self.copy_inner(frame, commands.clone()) };
        if result.is_err() {
            // Only the failure path waits synchronously, before asking the host
            // to republish its still-owned source buffer as CPU pixels.
            unsafe {
                self.gl.finish();
            }
            let _ = commands.try_send((16, sequence.to_be_bytes().to_vec()));
        }
        self.submit_us += started.elapsed().as_micros();
        result
    }
    unsafe fn copy_inner(
        &mut self,
        frame: Frame,
        commands: Commands,
    ) -> Result<Image, String> {
        if self.source.is_some() {
            return Err("GPU source is still in flight".into());
        }
        let gl = &self.gl;
        unsafe {
            let old_texture = gl.get_parameter_texture(glow::TEXTURE_BINDING_2D);
            let old_fbo = gl.get_parameter_framebuffer(glow::FRAMEBUFFER_BINDING);
            let result = (|| -> Result<Image, String> {
                let image = self.egl.import(&frame)?;
                let texture = match gl.create_texture() {
                    Ok(texture) => texture,
                    Err(error) => {
                        self.egl.destroy_image(image);
                        return Err(error);
                    }
                };
                gl.bind_texture(glow::TEXTURE_2D, Some(texture));
                self.egl.bind_image(image);
                let fbo = match gl.create_framebuffer() {
                    Ok(fbo) => fbo,
                    Err(error) => {
                        gl.delete_texture(texture);
                        self.egl.destroy_image(image);
                        return Err(error);
                    }
                };
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
                gl.framebuffer_texture_2d(
                    glow::FRAMEBUFFER,
                    glow::COLOR_ATTACHMENT0,
                    glow::TEXTURE_2D,
                    Some(texture),
                    0,
                );
                let imported_ok =
                    gl.check_framebuffer_status(glow::FRAMEBUFFER) == glow::FRAMEBUFFER_COMPLETE;
                // Keep all source resources until copy completion, including on errors.
                self.source = Some(Source {
                    frame,
                    image,
                    texture,
                    fbo,
                    fence: std::ptr::null_mut(),
                    commands,
                    ready: Arc::new(AtomicBool::new(false)),
                    async_ack: false,
                    counted: false,
                });
                if !imported_ok {
                    return Err("Imported WebKit texture is not framebuffer-readable".into());
                }
                let source = self.source.as_ref().unwrap();
                let size = (source.frame.width, source.frame.height);
                if self.size != size || self.outputs.iter().any(Option::is_none) {
                    self.size = size;
                    for texture in &mut self.outputs {
                        if texture.is_none() {
                            *texture = Some(gl.create_texture()?);
                        }
                        gl.bind_texture(glow::TEXTURE_2D, *texture);
                        gl.tex_parameter_i32(
                            glow::TEXTURE_2D,
                            glow::TEXTURE_MIN_FILTER,
                            glow::LINEAR as i32,
                        );
                        gl.tex_parameter_i32(
                            glow::TEXTURE_2D,
                            glow::TEXTURE_MAG_FILTER,
                            glow::LINEAR as i32,
                        );
                        gl.tex_parameter_i32(
                            glow::TEXTURE_2D,
                            glow::TEXTURE_WRAP_S,
                            glow::CLAMP_TO_EDGE as i32,
                        );
                        gl.tex_parameter_i32(
                            glow::TEXTURE_2D,
                            glow::TEXTURE_WRAP_T,
                            glow::CLAMP_TO_EDGE as i32,
                        );
                        gl.tex_image_2d(
                            glow::TEXTURE_2D,
                            0,
                            glow::RGBA as i32,
                            size.0 as i32,
                            size.1 as i32,
                            0,
                            glow::RGBA,
                            glow::UNSIGNED_BYTE,
                            glow::PixelUnpackData::Slice(None),
                        );
                    }
                }
                // Alternating IDs invalidate Slint's image cache every frame while
                // retaining reusable storage and same-context GPU ordering.
                self.output_index ^= 1;
                gl.bind_texture(glow::TEXTURE_2D, self.outputs[self.output_index]);
                gl.copy_tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    0,
                    0,
                    0,
                    0,
                    size.0 as i32,
                    size.1 as i32,
                );
                let error = gl.get_error();
                if error != glow::NO_ERROR {
                    return Err(format!("Web GPU copy failed: GL {error:#x}"));
                }
                self.source.as_mut().unwrap().fence = self.egl.fence()?;
                gl.flush();
                let source = self.source.as_mut().unwrap();
                if let Some(fence) = self.egl.export_fence(source.fence) {
                    source.async_ack = self.completion.submit(completion::Job {
                        fence,
                        ready: source.ready.clone(),
                        commands: source.commands.clone(),
                        sequence: source.frame.sequence,
                    });
                }
                Ok(BorrowedOpenGLTextureBuilder::new_gl_2d_rgba_texture(
                    self.outputs[self.output_index].unwrap().0,
                    (size.0, size.1).into(),
                )
                .build())
            })();
            gl.bind_framebuffer(glow::FRAMEBUFFER, old_fbo);
            gl.bind_texture(glow::TEXTURE_2D, old_texture);
            if result.is_err() {
                gl.finish();
                self.release(false);
            }
            result
        }
    }
    unsafe fn release(&mut self, ack: bool) {
        if let Some(source) = self.source.take() {
            unsafe {
                if !source.fence.is_null() {
                    self.egl.destroy_fence(source.fence);
                }
                self.gl.delete_framebuffer(source.fbo);
                self.gl.delete_texture(source.texture);
                self.egl.destroy_image(source.image);
                if ack && !source.async_ack {
                    let _ = source
                        .commands
                        .try_send((11, source.frame.sequence.to_be_bytes().to_vec()));
                }
            }
        }
    }
    pub fn needs_poll(&self) -> bool {
        self.source.as_ref().is_some_and(|source| !source.async_ack)
    }
    pub unsafe fn prepare(&mut self) {
        if self
            .source
            .as_ref()
            .is_some_and(|source| source.ready.load(Ordering::Acquire))
        {
            unsafe {
                self.release(false);
            }
        }
    }
    pub unsafe fn after_render(&mut self) -> Result<(), String> {
        if let Some(source) = self.source.as_mut() {
            if !source.counted {
                source.counted = true;
                self.frames += 1;
                if self.trace && self.since.elapsed().as_secs_f64() >= 2.0 {
                    eprintln!(
                        "agx-web: GPU bridge {:.1} fps; import/copy submit {:.2} ms/frame",
                        self.frames as f64 / self.since.elapsed().as_secs_f64(),
                        self.submit_us as f64 / self.frames.max(1) as f64 / 1000.0
                    );
                    self.since = Instant::now();
                    self.frames = 0;
                    self.submit_us = 0;
                }
            }
            if !source.async_ack && unsafe { self.egl.complete(source.fence)? } {
                unsafe {
                    self.release(true);
                }
            } else if source.ready.load(Ordering::Acquire) {
                unsafe {
                    self.release(false);
                }
            }
        }
        Ok(())
    }
    pub unsafe fn fallback(&mut self) {
        unsafe {
            self.gl.finish();
        }
        if let Some(source) = &self.source {
            let _ = source
                .commands
                .try_send((16, source.frame.sequence.to_be_bytes().to_vec()));
        }
        unsafe {
            self.release(false);
        }
    }
    pub unsafe fn destroy(&mut self) {
        unsafe {
            self.gl.finish();
            self.release(true);
            for texture in &mut self.outputs {
                if let Some(texture) = texture.take() {
                    self.gl.delete_texture(texture);
                }
            }
        }
    }
}
