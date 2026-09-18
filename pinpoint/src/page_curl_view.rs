use gtk::prelude::*;
use gtk::{gdk, glib};
use pinpoint_core::page_curl::{INDEX_COUNT, MeshVertex, build_mesh};
use std::cell::RefCell;
use std::ffi::{CString, c_char, c_float, c_int, c_uint, c_void};
use std::rc::Rc;

const GL_ARRAY_BUFFER: c_uint = 0x8892;
const GL_ELEMENT_ARRAY_BUFFER: c_uint = 0x8893;
const GL_STREAM_DRAW: c_uint = 0x88E0;
const GL_STATIC_DRAW: c_uint = 0x88E4;
const GL_UNSIGNED_INT: c_uint = 0x1405;
const GL_UNSIGNED_BYTE: c_uint = 0x1401;
const GL_FLOAT: c_uint = 0x1406;
const GL_FALSE: u8 = 0;
const GL_TRIANGLES: c_uint = 0x0004;
const GL_COLOR_BUFFER_BIT: c_uint = 0x0000_4000;
const GL_DEPTH_TEST: c_uint = 0x0B71;
const GL_CULL_FACE: c_uint = 0x0B44;
const GL_BLEND: c_uint = 0x0BE2;
const GL_TEXTURE_2D: c_uint = 0x0DE1;
const GL_TEXTURE0: c_uint = 0x84C0;
const GL_TEXTURE_MIN_FILTER: c_uint = 0x2801;
const GL_TEXTURE_MAG_FILTER: c_uint = 0x2800;
const GL_TEXTURE_WRAP_S: c_uint = 0x2802;
const GL_TEXTURE_WRAP_T: c_uint = 0x2803;
const GL_LINEAR: c_int = 0x2601;
const GL_CLAMP_TO_EDGE: c_int = 0x812F;
const GL_UNPACK_ALIGNMENT: c_uint = 0x0CF5;
const GL_UNPACK_ROW_LENGTH: c_uint = 0x0CF2;
const GL_RGBA: c_uint = 0x1908;
const GL_RGBA8: c_int = 0x8058;
const GL_VERTEX_SHADER: c_uint = 0x8B31;
const GL_FRAGMENT_SHADER: c_uint = 0x8B30;
const GL_COMPILE_STATUS: c_uint = 0x8B81;
const GL_LINK_STATUS: c_uint = 0x8B82;
const TEXTURE_SLOTS: usize = 4;

#[link(name = "epoxy")]
unsafe extern "C" {
    static epoxy_glActiveTexture: unsafe extern "C" fn(c_uint);
    static epoxy_glAttachShader: unsafe extern "C" fn(c_uint, c_uint);
    static epoxy_glBindBuffer: unsafe extern "C" fn(c_uint, c_uint);
    static epoxy_glBindTexture: unsafe extern "C" fn(c_uint, c_uint);
    static epoxy_glBindVertexArray: unsafe extern "C" fn(c_uint);
    static epoxy_glBufferData: unsafe extern "C" fn(c_uint, isize, *const c_void, c_uint);
    static epoxy_glBufferSubData: unsafe extern "C" fn(c_uint, isize, isize, *const c_void);
    static epoxy_glClear: unsafe extern "C" fn(c_uint);
    static epoxy_glClearColor: unsafe extern "C" fn(c_float, c_float, c_float, c_float);
    static epoxy_glCompileShader: unsafe extern "C" fn(c_uint);
    static epoxy_glCreateProgram: unsafe extern "C" fn() -> c_uint;
    static epoxy_glCreateShader: unsafe extern "C" fn(c_uint) -> c_uint;
    static epoxy_glDeleteBuffers: unsafe extern "C" fn(c_int, *const c_uint);
    static epoxy_glDeleteProgram: unsafe extern "C" fn(c_uint);
    static epoxy_glDeleteShader: unsafe extern "C" fn(c_uint);
    static epoxy_glDeleteTextures: unsafe extern "C" fn(c_int, *const c_uint);
    static epoxy_glDeleteVertexArrays: unsafe extern "C" fn(c_int, *const c_uint);
    static epoxy_glDisable: unsafe extern "C" fn(c_uint);
    static epoxy_glDrawElements: unsafe extern "C" fn(c_uint, c_int, c_uint, *const c_void);
    static epoxy_glEnableVertexAttribArray: unsafe extern "C" fn(c_uint);
    static epoxy_glGenBuffers: unsafe extern "C" fn(c_int, *mut c_uint);
    static epoxy_glGenTextures: unsafe extern "C" fn(c_int, *mut c_uint);
    static epoxy_glGenVertexArrays: unsafe extern "C" fn(c_int, *mut c_uint);
    static epoxy_glGetProgramInfoLog: unsafe extern "C" fn(c_uint, c_int, *mut c_int, *mut c_char);
    static epoxy_glGetProgramiv: unsafe extern "C" fn(c_uint, c_uint, *mut c_int);
    static epoxy_glGetShaderInfoLog: unsafe extern "C" fn(c_uint, c_int, *mut c_int, *mut c_char);
    static epoxy_glGetShaderiv: unsafe extern "C" fn(c_uint, c_uint, *mut c_int);
    static epoxy_glGetUniformLocation: unsafe extern "C" fn(c_uint, *const c_char) -> c_int;
    static epoxy_glLinkProgram: unsafe extern "C" fn(c_uint);
    static epoxy_glPixelStorei: unsafe extern "C" fn(c_uint, c_int);
    static epoxy_glShaderSource:
        unsafe extern "C" fn(c_uint, c_int, *const *const c_char, *const c_int);
    static epoxy_glTexImage2D: unsafe extern "C" fn(
        c_uint,
        c_int,
        c_int,
        c_int,
        c_int,
        c_int,
        c_uint,
        c_uint,
        *const c_void,
    );
    static epoxy_glTexParameteri: unsafe extern "C" fn(c_uint, c_uint, c_int);
    static epoxy_glUniform1i: unsafe extern "C" fn(c_int, c_int);
    static epoxy_glUseProgram: unsafe extern "C" fn(c_uint);
    static epoxy_glVertexAttribPointer:
        unsafe extern "C" fn(c_uint, c_int, c_uint, u8, c_int, *const c_void);
    static epoxy_glViewport: unsafe extern "C" fn(c_int, c_int, c_int, c_int);
}

struct Renderer {
    program: c_uint,
    texture_location: c_int,
    vertex_array: c_uint,
    vertex_buffer: c_uint,
    index_buffer: c_uint,
    flat_vertex_array: c_uint,
    flat_vertex_buffer: c_uint,
    flat_index_buffer: c_uint,
    slide_textures: [c_uint; TEXTURE_SLOTS],
    uploaded_slides: [Option<gdk::Texture>; TEXTURE_SLOTS],
    texture_last_used: [u64; TEXTURE_SLOTS],
    texture_use_clock: u64,
}

impl Renderer {
    fn compile_shader(kind: c_uint, source: &str) -> Result<c_uint, String> {
        let source = CString::new(source).map_err(|error| error.to_string())?;
        // SAFETY: GtkGLArea has made its current context active and the source
        // pointer remains valid for the duration of each OpenGL call.
        unsafe {
            let shader = epoxy_glCreateShader(kind);
            epoxy_glShaderSource(shader, 1, &source.as_ptr(), std::ptr::null());
            epoxy_glCompileShader(shader);
            let mut status = 0;
            epoxy_glGetShaderiv(shader, GL_COMPILE_STATUS, &mut status);
            if status == 0 {
                let mut log = [0_i8; 1024];
                epoxy_glGetShaderInfoLog(
                    shader,
                    log.len() as c_int,
                    std::ptr::null_mut(),
                    log.as_mut_ptr(),
                );
                epoxy_glDeleteShader(shader);
                return Err(format!(
                    "page-curl shader compilation failed: {}",
                    std::ffi::CStr::from_ptr(log.as_ptr()).to_string_lossy()
                ));
            }
            Ok(shader)
        }
    }

    fn configure_mesh(
        vertex_array: c_uint,
        vertex_buffer: c_uint,
        index_buffer: c_uint,
        vertices: Option<&[MeshVertex]>,
        indices: Option<&[u32]>,
        usage: c_uint,
    ) {
        let vertex_size = size_of::<MeshVertex>() * pinpoint_core::page_curl::VERTEX_COUNT;
        let index_size = size_of::<u32>() * INDEX_COUNT;
        // SAFETY: All object names belong to the current context. Optional
        // slices either provide the complete initialized buffer or a null
        // allocation that is populated before drawing.
        unsafe {
            epoxy_glBindVertexArray(vertex_array);
            epoxy_glBindBuffer(GL_ARRAY_BUFFER, vertex_buffer);
            epoxy_glBufferData(
                GL_ARRAY_BUFFER,
                vertex_size as isize,
                vertices.map_or(std::ptr::null(), |value| value.as_ptr().cast()),
                usage,
            );
            let stride = size_of::<MeshVertex>() as c_int;
            epoxy_glVertexAttribPointer(0, 2, GL_FLOAT, GL_FALSE, stride, std::ptr::null());
            epoxy_glVertexAttribPointer(
                1,
                2,
                GL_FLOAT,
                GL_FALSE,
                stride,
                (2 * size_of::<f32>()) as *const c_void,
            );
            epoxy_glVertexAttribPointer(
                2,
                1,
                GL_FLOAT,
                GL_FALSE,
                stride,
                (4 * size_of::<f32>()) as *const c_void,
            );
            epoxy_glEnableVertexAttribArray(0);
            epoxy_glEnableVertexAttribArray(1);
            epoxy_glEnableVertexAttribArray(2);
            epoxy_glBindBuffer(GL_ELEMENT_ARRAY_BUFFER, index_buffer);
            epoxy_glBufferData(
                GL_ELEMENT_ARRAY_BUFFER,
                index_size as isize,
                indices.map_or(std::ptr::null(), |value| value.as_ptr().cast()),
                usage,
            );
        }
    }

    fn new() -> Result<Self, String> {
        let vertex = Self::compile_shader(
            GL_VERTEX_SHADER,
            "#version 330 core\nlayout(location=0) in vec2 position; layout(location=1) in vec2 texture_coordinate; layout(location=2) in float lighting; out vec2 texture_position; out float shade; void main(){ gl_Position=vec4(position,0.0,1.0); texture_position=texture_coordinate; shade=lighting; }",
        )?;
        let fragment = Self::compile_shader(
            GL_FRAGMENT_SHADER,
            "#version 330 core\nuniform sampler2D page; in vec2 texture_position; in float shade; out vec4 color; void main(){ color=texture(page,texture_position)*vec4(shade,shade,shade,1.0); }",
        )?;
        // SAFETY: All names are created and configured in the current GLArea
        // context, and shader names remain valid until after linking.
        unsafe {
            let program = epoxy_glCreateProgram();
            epoxy_glAttachShader(program, vertex);
            epoxy_glAttachShader(program, fragment);
            epoxy_glLinkProgram(program);
            epoxy_glDeleteShader(vertex);
            epoxy_glDeleteShader(fragment);
            let mut status = 0;
            epoxy_glGetProgramiv(program, GL_LINK_STATUS, &mut status);
            if status == 0 {
                let mut log = [0_i8; 1024];
                epoxy_glGetProgramInfoLog(
                    program,
                    log.len() as c_int,
                    std::ptr::null_mut(),
                    log.as_mut_ptr(),
                );
                epoxy_glDeleteProgram(program);
                return Err(format!(
                    "page-curl shader link failed: {}",
                    std::ffi::CStr::from_ptr(log.as_ptr()).to_string_lossy()
                ));
            }

            let texture_name = CString::new("page").expect("uniform name contains no NUL");
            let texture_location = epoxy_glGetUniformLocation(program, texture_name.as_ptr());
            let mut vertex_arrays = [0, 0];
            let mut buffers = [0, 0, 0, 0];
            let mut slide_textures = [0; TEXTURE_SLOTS];
            epoxy_glGenVertexArrays(2, vertex_arrays.as_mut_ptr());
            epoxy_glGenBuffers(4, buffers.as_mut_ptr());
            epoxy_glGenTextures(TEXTURE_SLOTS as c_int, slide_textures.as_mut_ptr());
            Self::configure_mesh(
                vertex_arrays[0],
                buffers[0],
                buffers[1],
                None,
                None,
                GL_STREAM_DRAW,
            );
            let (flat_vertices, flat_indices) = build_mesh(1.0, 1.0, 0.0, 0.0);
            Self::configure_mesh(
                vertex_arrays[1],
                buffers[2],
                buffers[3],
                Some(&flat_vertices),
                Some(&flat_indices),
                GL_STATIC_DRAW,
            );
            epoxy_glBindVertexArray(0);
            Ok(Self {
                program,
                texture_location,
                vertex_array: vertex_arrays[0],
                vertex_buffer: buffers[0],
                index_buffer: buffers[1],
                flat_vertex_array: vertex_arrays[1],
                flat_vertex_buffer: buffers[2],
                flat_index_buffer: buffers[3],
                slide_textures,
                uploaded_slides: [None, None, None, None],
                texture_last_used: [0; TEXTURE_SLOTS],
                texture_use_clock: 0,
            })
        }
    }

    fn upload_texture(&mut self, texture: &gdk::Texture) -> Result<usize, String> {
        self.texture_use_clock = self.texture_use_clock.wrapping_add(1);
        if let Some(index) = self
            .uploaded_slides
            .iter()
            .position(|uploaded| uploaded.as_ref() == Some(texture))
        {
            self.texture_last_used[index] = self.texture_use_clock;
            return Ok(index);
        }
        let index = self
            .uploaded_slides
            .iter()
            .position(Option::is_none)
            .unwrap_or_else(|| {
                self.texture_last_used
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, used)| *used)
                    .map_or(0, |(index, _)| index)
            });
        let mut downloader = gdk::TextureDownloader::new(texture);
        downloader.set_format(gdk::MemoryFormat::R8g8b8a8Premultiplied);
        let (bytes, stride) = downloader.download_bytes();
        let width = texture.width();
        let height = texture.height();
        let required = stride
            .checked_mul(height as usize)
            .ok_or_else(|| "page-curl texture dimensions overflowed".to_owned())?;
        if width <= 0
            || height <= 0
            || stride < width as usize * 4
            || stride % 4 != 0
            || stride / 4 > c_int::MAX as usize
            || required > bytes.len()
        {
            return Err("page-curl texture has an invalid pixel layout".to_owned());
        }
        // SAFETY: `bytes` contains at least stride*height bytes and remains
        // alive until the synchronous upload has completed.
        unsafe {
            epoxy_glBindTexture(GL_TEXTURE_2D, self.slide_textures[index]);
            epoxy_glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
            epoxy_glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
            epoxy_glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
            epoxy_glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
            epoxy_glPixelStorei(GL_UNPACK_ALIGNMENT, 1);
            epoxy_glPixelStorei(GL_UNPACK_ROW_LENGTH, (stride / 4) as c_int);
            epoxy_glTexImage2D(
                GL_TEXTURE_2D,
                0,
                GL_RGBA8,
                width,
                height,
                0,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                bytes.as_ref().as_ptr().cast(),
            );
            epoxy_glPixelStorei(GL_UNPACK_ROW_LENGTH, 0);
        }
        self.uploaded_slides[index] = Some(texture.clone());
        self.texture_last_used[index] = self.texture_use_clock;
        Ok(index)
    }

    fn draw_page(&self, texture: usize, period: f64, angle: f64, width: f32, height: f32) {
        // SAFETY: The configured arrays and buffers remain owned by this
        // renderer. Dynamic slices live until each synchronous upload returns.
        unsafe {
            if period <= f64::EPSILON {
                epoxy_glBindVertexArray(self.flat_vertex_array);
            } else {
                let (vertices, indices) = build_mesh(width, height, period, angle);
                epoxy_glBindVertexArray(self.vertex_array);
                epoxy_glBindBuffer(GL_ARRAY_BUFFER, self.vertex_buffer);
                epoxy_glBufferSubData(
                    GL_ARRAY_BUFFER,
                    0,
                    std::mem::size_of_val(vertices.as_slice()) as isize,
                    vertices.as_ptr().cast(),
                );
                epoxy_glBindBuffer(GL_ELEMENT_ARRAY_BUFFER, self.index_buffer);
                epoxy_glBufferSubData(
                    GL_ELEMENT_ARRAY_BUFFER,
                    0,
                    std::mem::size_of_val(indices.as_slice()) as isize,
                    indices.as_ptr().cast(),
                );
            }
            epoxy_glActiveTexture(GL_TEXTURE0);
            epoxy_glBindTexture(GL_TEXTURE_2D, self.slide_textures[texture]);
            epoxy_glUniform1i(self.texture_location, 0);
            epoxy_glDrawElements(
                GL_TRIANGLES,
                INDEX_COUNT as c_int,
                GL_UNSIGNED_INT,
                std::ptr::null(),
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render(
        &mut self,
        slides: [&gdk::Texture; 2],
        periods: [f64; 2],
        angles: [f64; 2],
        backwards: bool,
        width: i32,
        height: i32,
        scale: i32,
    ) -> Result<(), String> {
        let textures = [
            self.upload_texture(slides[0])?,
            self.upload_texture(slides[1])?,
        ];
        let viewport_width = width
            .checked_mul(scale)
            .ok_or_else(|| "page-curl viewport width overflowed".to_owned())?;
        let viewport_height = height
            .checked_mul(scale)
            .ok_or_else(|| "page-curl viewport height overflowed".to_owned())?;
        // SAFETY: Rendering occurs only while the GLArea context is current.
        unsafe {
            epoxy_glViewport(0, 0, viewport_width, viewport_height);
            epoxy_glDisable(GL_DEPTH_TEST);
            epoxy_glDisable(GL_CULL_FACE);
            epoxy_glDisable(GL_BLEND);
            epoxy_glClearColor(0.0, 0.0, 0.0, 1.0);
            epoxy_glClear(GL_COLOR_BUFFER_BIT);
            epoxy_glUseProgram(self.program);
        }
        let order = if backwards { [0, 1] } else { [1, 0] };
        for slide in order {
            self.draw_page(
                textures[slide],
                periods[slide],
                angles[slide],
                width as f32,
                height as f32,
            );
        }
        // SAFETY: Resetting bindings does not outlive or alias Rust values.
        unsafe {
            epoxy_glBindVertexArray(0);
            epoxy_glUseProgram(0);
        }
        Ok(())
    }

    fn destroy(self) {
        let buffers = [
            self.vertex_buffer,
            self.index_buffer,
            self.flat_vertex_buffer,
            self.flat_index_buffer,
        ];
        let vertex_arrays = [self.vertex_array, self.flat_vertex_array];
        // SAFETY: Called during GLArea unrealize with its context current. Each
        // name was created by this renderer and is deleted exactly once.
        unsafe {
            epoxy_glDeleteTextures(TEXTURE_SLOTS as c_int, self.slide_textures.as_ptr());
            epoxy_glDeleteBuffers(4, buffers.as_ptr());
            epoxy_glDeleteVertexArrays(2, vertex_arrays.as_ptr());
            epoxy_glDeleteProgram(self.program);
        }
    }
}

#[derive(Default)]
struct ViewState {
    renderer: Option<Renderer>,
    slides: [Option<gdk::Texture>; 2],
    periods: [f64; 2],
    angles: [f64; 2],
    backwards: bool,
    reported_error: bool,
}

#[derive(Clone)]
pub struct PageCurlView {
    area: gtk::GLArea,
    state: Rc<RefCell<ViewState>>,
}

impl PageCurlView {
    pub fn new() -> Self {
        let area = gtk::GLArea::builder().hexpand(true).vexpand(true).build();
        area.set_allowed_apis(gdk::GLAPI::GL);
        area.set_required_version(3, 3);
        area.set_auto_render(false);
        let state = Rc::new(RefCell::new(ViewState::default()));

        area.connect_realize(glib::clone!(
            #[strong]
            state,
            move |area| {
                area.make_current();
                if let Some(error) = area.error() {
                    eprintln!("PINPOINT PAGE CURL GL context failed: {error}");
                    state.borrow_mut().reported_error = true;
                    return;
                }
                match Renderer::new() {
                    Ok(renderer) => {
                        state.borrow_mut().renderer = Some(renderer);
                        eprintln!("PINPOINT PAGE CURL GL ready mesh=32x32 shading=interpolated");
                    }
                    Err(error) => {
                        eprintln!("PINPOINT PAGE CURL GL failed: {error}");
                        state.borrow_mut().reported_error = true;
                    }
                }
            }
        ));
        area.connect_render(glib::clone!(
            #[strong]
            state,
            move |area, _| {
                let (slides, periods, angles, backwards) = {
                    let state = state.borrow();
                    let (Some(previous), Some(current)) =
                        (state.slides[0].clone(), state.slides[1].clone())
                    else {
                        return glib::Propagation::Stop;
                    };
                    (
                        [previous, current],
                        state.periods,
                        state.angles,
                        state.backwards,
                    )
                };
                let result = {
                    let mut state = state.borrow_mut();
                    state.renderer.as_mut().map_or_else(
                        || Err("OpenGL renderer is unavailable".to_owned()),
                        |renderer| {
                            renderer.render(
                                [&slides[0], &slides[1]],
                                periods,
                                angles,
                                backwards,
                                area.width(),
                                area.height(),
                                area.scale_factor(),
                            )
                        },
                    )
                };
                if let Err(error) = result {
                    let mut state = state.borrow_mut();
                    if !state.reported_error {
                        eprintln!("PINPOINT PAGE CURL GL failed: {error}");
                        state.reported_error = true;
                    }
                }
                glib::Propagation::Stop
            }
        ));
        area.connect_unrealize(glib::clone!(
            #[strong]
            state,
            move |area| {
                area.make_current();
                if area.error().is_none()
                    && let Some(renderer) = state.borrow_mut().renderer.take()
                {
                    renderer.destroy();
                }
                state.borrow_mut().renderer = None;
            }
        ));
        Self { area, state }
    }

    pub fn widget(&self) -> &gtk::GLArea {
        &self.area
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_transition(
        &self,
        previous: &gdk::Texture,
        current: &gdk::Texture,
        previous_period: f64,
        previous_angle: f64,
        current_period: f64,
        current_angle: f64,
        backwards: bool,
    ) {
        let mut state = self.state.borrow_mut();
        state.slides = [Some(previous.clone()), Some(current.clone())];
        state.periods = [previous_period, current_period];
        state.angles = [previous_angle, current_angle];
        state.backwards = backwards;
        drop(state);
        self.area.queue_render();
    }

    pub fn prewarm_textures(&self, pairs: Vec<(gdk::Texture, gdk::Texture)>) {
        if pairs.is_empty() || !self.area.is_realized() {
            return;
        }
        self.area.make_current();
        if self.area.error().is_some() {
            return;
        }
        let mut state = self.state.borrow_mut();
        let Some(renderer) = state.renderer.as_mut() else {
            return;
        };
        for (previous, current) in pairs {
            if let Err(error) = renderer
                .upload_texture(&previous)
                .and_then(|_| renderer.upload_texture(&current))
            {
                if !state.reported_error {
                    eprintln!("PINPOINT PAGE CURL prewarm failed: {error}");
                    state.reported_error = true;
                }
                break;
            }
        }
    }

    pub fn hide(&self) {
        let mut state = self.state.borrow_mut();
        state.slides = [None, None];
    }

    pub fn clear(&self) {
        let mut state = self.state.borrow_mut();
        state.slides = [None, None];
        if let Some(renderer) = state.renderer.as_mut() {
            renderer.uploaded_slides = [None, None, None, None];
            renderer.texture_last_used = [0; TEXTURE_SLOTS];
            renderer.texture_use_clock = 0;
        }
    }
}
