use std::{
    fs::read_to_string,
    ops::{Add, Mul, Sub},
    sync::mpsc,
    thread,
    time::Duration,
};

use femtovg::{renderer::OpenGl, Canvas, Paint, Path, Transform2D};
use glutin::{
    config::ConfigTemplateBuilder,
    context::{ContextApi, ContextAttributesBuilder},
    display::GetGlDisplay,
    prelude::{GlDisplay, NotCurrentGlContextSurfaceAccessor},
    surface::{GlSurface, SurfaceAttributesBuilder},
};
use glutin_winit::{DisplayBuilder, GlWindow};
use raw_window_handle::HasRawWindowHandle;
use rlua::{Context, FromLuaMulti, Function, Lua, MultiValue, Result, Table};
use winit::{dpi::PhysicalSize, event_loop::EventLoop, window::WindowBuilder};

fn load_file<'lua>(ctx: Context<'lua>, name: &str) -> Result<Table<'lua>> {
    ctx.load(&read_to_string("luanim/src/".to_owned() + name + ".lua").unwrap())
        .set_name(name)?
        .eval::<Table>()
}

fn load_libs(ctx: Context) -> Result<()> {
    let globals = ctx.globals();
    globals.set("ir", load_file(ctx, "ir")?)?;

    globals.set("tweens", load_file(ctx, "tweens")?)?;
    globals.set("vector", load_file(ctx, "vector")?)?;
    globals.set("signal", load_file(ctx, "signal")?)?;
    globals.set("luanim", load_file(ctx, "luanim")?)?;

    globals.set("shapes", load_file(ctx, "shapes")?)?;
    Ok(())
}

const WIDTH: usize = 1920;
const HEIGHT: usize = 1080;
const TEXT_SCALE: f32 = 8.0 / 15.0;

struct Screen {
    canvas: Canvas<OpenGl>,
    transform_stack: Vec<Mat3>,
    line_width: f32,
    path: Option<Path>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Mat3 {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
}

impl Mat3 {
    fn new(a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) -> Mat3 {
        Mat3 { a, b, c, d, e, f }
    }
}

impl From<Mat3> for Transform2D {
    fn from(value: Mat3) -> Self {
        Transform2D([value.a, value.b, value.c, value.d, value.e, value.f])
    }
}

impl Mul<Mat3> for Mat3 {
    type Output = Mat3;

    fn mul(self, b: Mat3) -> Self::Output {
        let a = self;
        Mat3::new(
            a.a * b.a + a.c * b.b,
            a.b * b.a + a.d * b.b,
            a.a * b.c + a.c * b.d,
            a.b * b.c + a.d * b.d,
            a.a * b.e + a.c * b.f + a.e,
            a.b * b.e + a.d * b.f + a.f,
        )
    }
}

impl Mul<Vec2> for Mat3 {
    type Output = Vec2;

    fn mul(self, b: Vec2) -> Self::Output {
        let a = self;
        Vec2::new(a.a * b.x + a.c * b.y + a.e, a.b * b.x + a.d * b.y + a.f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Vec2 {
    x: f32,
    y: f32,
}

impl Vec2 {
    fn new(x: f32, y: f32) -> Vec2 {
        Vec2 { x, y }
    }
    fn len_squared(self) -> f32 {
        self.x * self.x + self.y * self.y
    }
}

impl Add<Vec2> for Vec2 {
    type Output = Vec2;

    fn add(self, rhs: Vec2) -> Self::Output {
        Vec2::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl Sub<Vec2> for Vec2 {
    type Output = Vec2;

    fn sub(self, rhs: Vec2) -> Self::Output {
        Vec2::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl Mul<Vec2> for f32 {
    type Output = Vec2;

    fn mul(self, rhs: Vec2) -> Self::Output {
        Vec2::new(self * rhs.x, self * rhs.y)
    }
}

impl Screen {
    fn point_at(&self, x: f32, y: f32) -> Vec2 {
        *self.transform_stack.last().unwrap() * Vec2::new(x, y)
    }
    fn draw_circle(&mut self, center: Vec2, radius: f32) {
        let mut circle = Path::new();
        circle.circle(center.x, center.y, radius);
        self.canvas
            .fill_path(&mut circle, &Paint::color(femtovg::Color::white()))
    }
    fn draw_ellipse(&mut self, focus1: Vec2, focus2: Vec2, sum: f32) {
        if (focus2 - focus1).len_squared() < 1.0 {
            self.draw_circle(focus1, sum / 2.0);
            return;
        } else {
            todo!("{:?} {:?}", focus1, focus2);
        }
    }
}

unsafe fn any_as_u8_slice<T: Sized>(p: &T) -> &[u8] {
    ::core::slice::from_raw_parts((p as *const T) as *const u8, ::core::mem::size_of::<T>())
}

#[derive(Debug)]
enum Draw {
    Start(Mat3),
    Circle(Vec2, f32),
    Point(Vec2, f32),
    End,
    Text(Vec2, f32, String),
    LineWidth(f32),
    PathStart(Vec2),
    Line(Vec2),
    PathClose,
    PathEnd,
}

#[derive(Debug)]
enum Request {
    Draw(Vec<Draw>),
    Measure(String, Option<String>),
}

fn main() -> Result<()> {
    let (tx_size, rx_size) = mpsc::sync_channel(0);
    let (tx_instrs, rx_instrs) = mpsc::channel();
    let (tx_frame, rx_frame) = mpsc::channel();

    thread::spawn(move || -> Result<()> {
        let lua = unsafe { Lua::new_with_debug() };

        lua.context(move |ctx| -> Result<()> {
            let globals = ctx.globals();

            let canvas = ctx.create_table()?;
            let tx_measure = tx_instrs.clone();
            canvas.set(
                "measure",
                ctx.create_function(move |_, (text, font): (String, Option<String>)| {
                    tx_measure.send(Request::Measure(text, font)).unwrap();
                    Ok(rx_size.recv().unwrap())
                })?,
            )?;
            canvas.set("preferred_fps", ctx.create_function(|_, ()| Ok(60))?)?;
            globals.set("canvas", canvas)?;

            load_libs(ctx)?;

            let anim = ctx
                .load(&read_to_string("luanim/examples/prime_plot.notlua").unwrap())
                .eval::<Function>()?;

            let (tx_instr, rx_instr) = mpsc::channel();

            let emit = ctx.create_function_mut(move |ctx, (instr, args): (u8, MultiValue)| {
                let instr = match instr {
                    0 => {
                        let (_, _, a, b, c, d, e, f): (String, bool, f32, f32, f32, f32, f32, f32) =
                            FromLuaMulti::from_lua_multi(args, ctx)?;
                        Draw::Start(Mat3::new(a, b, c, d, e, f))
                    }
                    1 => Draw::End,
                    3 => Draw::LineWidth(FromLuaMulti::from_lua_multi(args, ctx)?),
                    4 => {
                        let (x, y, r) = FromLuaMulti::from_lua_multi(args, ctx)?;
                        Draw::Point(Vec2::new(x, y), r)
                    }
                    7 => {
                        let (x, y) = FromLuaMulti::from_lua_multi(args, ctx)?;
                        Draw::PathStart(Vec2::new(x, y))
                    }
                    9 => {
                        let (x, y) = FromLuaMulti::from_lua_multi(args, ctx)?;
                        Draw::Line(Vec2::new(x, y))
                    }
                    10 => Draw::PathClose,
                    20 => Draw::PathEnd,
                    13 => {
                        let (x, y, size, text) = FromLuaMulti::from_lua_multi(args, ctx)?;
                        Draw::Text(Vec2::new(x, y), size, text)
                    }
                    19 => {
                        let (x, y, r) = FromLuaMulti::from_lua_multi(args, ctx)?;
                        Draw::Circle(Vec2::new(x, y), r)
                    }
                    _ => todo!("{}", instr),
                };

                tx_instr.send(instr).unwrap();
                Ok(())
            })?;

            loop {
                let frame: usize = rx_frame.recv().unwrap();
                anim.call((frame, emit.clone()))?;

                let mut instrs = Vec::new();
                while let Ok(instr) = rx_instr.try_recv() {
                    instrs.push(instr);
                }

                tx_instrs.send(Request::Draw(instrs)).unwrap();
            }
        })?;
        Ok(())
    });

    let el = EventLoop::new();
    let (window, config) = DisplayBuilder::new()
        .with_window_builder(Some(
            WindowBuilder::new()
                .with_title("shellkick")
                .with_inner_size(PhysicalSize::new(WIDTH as u32, HEIGHT as u32))
                .with_resizable(false),
        ))
        .build(&el, ConfigTemplateBuilder::new(), |mut it| {
            it.next().unwrap()
        })
        .unwrap();

    let window = window.unwrap();
    let attrs = window.build_surface_attributes(SurfaceAttributesBuilder::new());

    let display = config.display();
    let surface = unsafe { display.create_window_surface(&config, &attrs).unwrap() };

    let gl_context = unsafe {
        display
            .create_context(
                &config,
                &ContextAttributesBuilder::new()
                    .with_context_api(ContextApi::OpenGl(None))
                    .build(Some(window.raw_window_handle())),
            )
            .unwrap()
            .make_current(&surface)
            .unwrap()
    };

    let opengl = OpenGl::new_from_glutin_display(&config.display()).unwrap();
    let canvas = Canvas::new(opengl).unwrap();

    let mut screen = Screen {
        canvas,
        transform_stack: vec![Mat3::new(
            WIDTH as f32 / 2.0 / 256.0,
            0.0,
            0.0,
            WIDTH as f32 / 2.0 / 256.0,
            WIDTH as f32 / 2.0,
            HEIGHT as f32 / 2.0,
        )],
        line_width: 1.0,
        path: None,
    };

    screen.canvas.set_size(WIDTH as u32, HEIGHT as u32, 1.0);
    screen.canvas.add_font("res/pressstart.ttf").unwrap();

    let mut frame = 0;

    loop {
        tx_frame.send(frame).unwrap();
        frame += 1;

        screen
            .canvas
            .clear_rect(0, 0, WIDTH as u32, HEIGHT as u32, femtovg::Color::black());

        let instrs = loop {
            match rx_instrs.recv().unwrap() {
                Request::Draw(instrs) => break instrs,
                Request::Measure(string, _) => {
                    tx_size
                        .send(
                            screen
                                .canvas
                                .measure_text(
                                    0.0,
                                    0.0,
                                    string,
                                    &Paint::color(femtovg::Color::black()),
                                )
                                .unwrap()
                                .width()
                                * TEXT_SCALE,
                        )
                        .unwrap();
                }
            }
        };

        for instr in instrs {
            match instr {
                Draw::Start(mat) => {
                    let last = *screen.transform_stack.last().unwrap();
                    let new = last * mat;
                    screen.transform_stack.push(new);
                }
                Draw::Circle(p, r) => {
                    let middle = screen.point_at(p.x, p.y);
                    let vert = screen.point_at(p.x, p.y + r) - middle;
                    let horz = screen.point_at(p.x + r, p.y) - middle;

                    let a2 = horz.len_squared();
                    let b2 = vert.len_squared();

                    if a2 > b2 {
                        let sum = 2.0 * a2.sqrt();

                        let eccentricity = (1.0 - b2 / a2).sqrt();

                        let focus1 = middle - eccentricity * horz;
                        let focus2 = middle + eccentricity * horz;

                        screen.draw_ellipse(focus1, focus2, sum);
                    } else {
                        let sum = 2.0 * b2.sqrt();

                        let eccentricity = (1.0 - a2 / b2).sqrt();

                        let focus1 = middle - eccentricity * vert;
                        let focus2 = middle + eccentricity * vert;

                        screen.draw_ellipse(focus1, focus2, sum);
                    }
                }
                Draw::End => {
                    screen.transform_stack.pop();
                }
                Draw::Text(p, size, text) => {
                    let trans = *screen.transform_stack.last().unwrap();
                    let rough_scale = (Vec2::new(trans.a, trans.b).len_squared().sqrt()
                        + Vec2::new(trans.c, trans.d).len_squared().sqrt())
                        / 2.0;
                    let font_size = size * TEXT_SCALE * 16.0 * rough_scale;

                    screen.canvas.set_transform(&trans.into());
                    screen.canvas.scale(1.0 / rough_scale, 1.0 / rough_scale);
                    screen
                        .canvas
                        .fill_text(
                            p.x,
                            p.y,
                            text,
                            &Paint::color(femtovg::Color::white()).with_font_size(font_size),
                        )
                        .unwrap();
                    screen.canvas.reset_transform();
                }
                Draw::LineWidth(width) => screen.line_width = width * screen.transform_stack[0].a,
                Draw::Point(p, r) => {
                    let middle = screen.point_at(p.x, p.y);
                    let radius = screen.transform_stack[0].a * r;
                    screen.draw_circle(middle, radius);
                }
                Draw::PathStart(p) => {
                    let p = screen.point_at(p.x, p.y);
                    let mut path = Path::new();
                    path.move_to(p.x, p.y);
                    screen.path = Some(path);
                }
                Draw::Line(p) => {
                    let p = screen.point_at(p.x, p.y);
                    screen.path.as_mut().unwrap().line_to(p.x, p.y);
                }
                Draw::PathClose => screen.path.as_mut().unwrap().close(),
                Draw::PathEnd => {
                    let mut path = screen.path.take().unwrap();
                    screen.canvas.stroke_path(
                        &mut path,
                        &Paint::color(femtovg::Color::white()).with_line_width(screen.line_width),
                    );
                }
            }
        }

        screen.canvas.flush();
        surface.swap_buffers(&gl_context).unwrap();

        ::std::thread::sleep(Duration::new(0, 1_000_000_000u32 / 60));
    }
}
