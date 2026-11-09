//! Tiny WebGL2 batch renderer: colored triangles in logical W x H space.

use crate::config::{H, W};
use crate::math::Vec2;
use wasm_bindgen::JsCast;
use web_sys::{
    HtmlCanvasElement, WebGl2RenderingContext as GL, WebGlBuffer, WebGlProgram, WebGlShader,
    WebGlUniformLocation, WebGlVertexArrayObject,
};

pub type Color = [f32; 4];

const VS: &str = r#"#version 300 es
in vec2 a_pos;
in vec4 a_col;
uniform vec2 u_res;
out vec4 v_col;
void main() {
    vec2 c = a_pos / u_res * 2.0 - 1.0;
    gl_Position = vec4(c.x, -c.y, 0.0, 1.0);
    v_col = a_col;
}"#;

const FS: &str = r#"#version 300 es
precision mediump float;
in vec4 v_col;
out vec4 o_col;
void main() { o_col = v_col; }"#;

pub struct Renderer {
    canvas: HtmlCanvasElement,
    gl: GL,
    program: WebGlProgram,
    vao: WebGlVertexArrayObject,
    buffer: WebGlBuffer,
    u_res: WebGlUniformLocation,
    verts: Vec<f32>,
}

fn compile(gl: &GL, kind: u32, src: &str) -> WebGlShader {
    let s = gl.create_shader(kind).expect("createShader failed");
    gl.shader_source(&s, src);
    gl.compile_shader(&s);
    if !gl.get_shader_parameter(&s, GL::COMPILE_STATUS).as_bool().unwrap_or(false) {
        panic!("shader compile error: {}", gl.get_shader_info_log(&s).unwrap_or_default());
    }
    s
}

impl Renderer {
    pub fn new(canvas: &HtmlCanvasElement) -> Self {
        let gl: GL = canvas
            .get_context("webgl2")
            .expect("getContext threw")
            .expect("WebGL2 is not supported")
            .dyn_into()
            .expect("context is not WebGL2");
        let program = gl.create_program().expect("createProgram failed");
        gl.attach_shader(&program, &compile(&gl, GL::VERTEX_SHADER, VS));
        gl.attach_shader(&program, &compile(&gl, GL::FRAGMENT_SHADER, FS));
        gl.link_program(&program);
        if !gl.get_program_parameter(&program, GL::LINK_STATUS).as_bool().unwrap_or(false) {
            panic!("program link error: {}", gl.get_program_info_log(&program).unwrap_or_default());
        }
        let vao = gl.create_vertex_array().expect("createVertexArray failed");
        gl.bind_vertex_array(Some(&vao));
        let buffer = gl.create_buffer().expect("createBuffer failed");
        gl.bind_buffer(GL::ARRAY_BUFFER, Some(&buffer));
        for (name, size, offset) in [("a_pos", 2, 0), ("a_col", 4, 8)] {
            let loc = gl.get_attrib_location(&program, name);
            assert!(loc >= 0, "attribute {name} not found");
            gl.enable_vertex_attrib_array(loc as u32);
            gl.vertex_attrib_pointer_with_i32(loc as u32, size, GL::FLOAT, false, 24, offset);
        }
        let u_res = gl.get_uniform_location(&program, "u_res").expect("uniform u_res not found");
        gl.enable(GL::BLEND);
        gl.blend_func(GL::SRC_ALPHA, GL::ONE_MINUS_SRC_ALPHA);
        Renderer { canvas: canvas.clone(), gl, program, vao, buffer, u_res, verts: Vec::with_capacity(1 << 16) }
    }

    pub fn begin(&mut self) {
        self.verts.clear();
        let dpr = web_sys::window().expect("no window").device_pixel_ratio();
        let w = (self.canvas.client_width() as f64 * dpr).round() as u32;
        let h = (self.canvas.client_height() as f64 * dpr).round() as u32;
        if self.canvas.width() != w || self.canvas.height() != h {
            self.canvas.set_width(w);
            self.canvas.set_height(h);
        }
        self.gl.viewport(0, 0, w as i32, h as i32);
    }

    pub fn flush(&mut self, clear: Color) {
        let gl = &self.gl;
        gl.clear_color(clear[0], clear[1], clear[2], clear[3]);
        gl.clear(GL::COLOR_BUFFER_BIT);
        gl.use_program(Some(&self.program));
        gl.uniform2f(Some(&self.u_res), W, H);
        gl.bind_vertex_array(Some(&self.vao));
        gl.bind_buffer(GL::ARRAY_BUFFER, Some(&self.buffer));
        // SAFETY: the view is consumed by bufferData before any wasm allocation can happen.
        unsafe {
            let view = js_sys::Float32Array::view(&self.verts);
            gl.buffer_data_with_array_buffer_view(GL::ARRAY_BUFFER, &view, GL::DYNAMIC_DRAW);
        }
        gl.draw_arrays(GL::TRIANGLES, 0, (self.verts.len() / 6) as i32);
    }

    fn v(&mut self, x: f32, y: f32, c: Color) {
        self.verts.extend_from_slice(&[x, y, c[0], c[1], c[2], c[3]]);
    }

    pub fn tri(&mut self, a: Vec2, b: Vec2, c: Vec2, col: Color) {
        self.v(a.x, a.y, col);
        self.v(b.x, b.y, col);
        self.v(c.x, c.y, col);
    }

    /// Top-left anchored rectangle.
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: Color) {
        let (a, b, cc, d) = (Vec2::new(x, y), Vec2::new(x + w, y), Vec2::new(x + w, y + h), Vec2::new(x, y + h));
        self.tri(a, b, cc, c);
        self.tri(a, cc, d, c);
    }

    pub fn rect_c(&mut self, p: Vec2, w: f32, h: f32, c: Color) {
        self.rect(p.x - w / 2.0, p.y - h / 2.0, w, h, c);
    }

    pub fn frame_c(&mut self, p: Vec2, w: f32, h: f32, t: f32, c: Color) {
        let (x, y) = (p.x - w / 2.0, p.y - h / 2.0);
        self.rect(x, y, w, t, c);
        self.rect(x, y + h - t, w, t, c);
        self.rect(x, y + t, t, h - 2.0 * t, c);
        self.rect(x + w - t, y + t, t, h - 2.0 * t, c);
    }

    pub fn circle(&mut self, p: Vec2, r: f32, c: Color) {
        const N: usize = 24;
        for i in 0..N {
            let a0 = i as f32 / N as f32 * std::f32::consts::TAU;
            let a1 = (i + 1) as f32 / N as f32 * std::f32::consts::TAU;
            self.tri(p, p + Vec2::new(a0.cos(), a0.sin()) * r, p + Vec2::new(a1.cos(), a1.sin()) * r, c);
        }
    }

    pub fn ring(&mut self, p: Vec2, r: f32, t: f32, c: Color) {
        const N: usize = 32;
        for i in 0..N {
            let a0 = i as f32 / N as f32 * std::f32::consts::TAU;
            let a1 = (i + 1) as f32 / N as f32 * std::f32::consts::TAU;
            let (d0, d1) = (Vec2::new(a0.cos(), a0.sin()), Vec2::new(a1.cos(), a1.sin()));
            let (o0, o1, i0, i1) = (p + d0 * r, p + d1 * r, p + d0 * (r - t), p + d1 * (r - t));
            self.tri(o0, o1, i1, c);
            self.tri(o0, i1, i0, c);
        }
    }

    pub fn line(&mut self, a: Vec2, b: Vec2, thick: f32, c: Color) {
        let d = (b - a).norm();
        let n = Vec2::new(-d.y, d.x) * (thick / 2.0);
        self.tri(a + n, b + n, b - n, c);
        self.tri(a + n, b - n, a - n, c);
    }
}
