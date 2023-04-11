use std::{
    cell::RefCell,
    fs::read_to_string,
    ops::{Add, Mul, Sub},
};

use femtovg::{Canvas, Color, Paint, Path, Renderer, Transform2D};
use rlua::{
    Context, FromLua, FromLuaMulti, Function, Lua, MultiValue, Result, Scope, Table, ToLua, Value,
};

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

const TEXT_SCALE: f32 = 8.0 / 15.0;

pub struct Screen<T: Renderer> {
    transform_stack: Vec<Mat3>,
    path: Option<Path>,

    pub line_width: f32,
    pub canvas: Canvas<T>,
}

pub struct Animation<T: Renderer> {
    custom:
        Box<dyn for<'lua> Fn(Context<'lua>, u8, MultiValue<'lua>, &mut Screen<T>) -> Result<()>>,
    lua: Lua,
    screen: Screen<T>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat3 {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
}

impl Mat3 {
    pub fn new(a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) -> Mat3 {
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
pub struct Vec2 {
    x: f32,
    y: f32,
}

impl Vec2 {
    pub fn new(x: f32, y: f32) -> Vec2 {
        Vec2 { x, y }
    }
    pub fn len_squared(self) -> f32 {
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

pub struct AnimationValues<'lua>(Context<'lua>, Table<'lua>);

impl<'lua> AnimationValues<'lua> {
    pub fn set(&self, key: impl ToLua<'lua>, value: impl ToLua<'lua>) -> Result<()> {
        let signal: Function = self.1.get(key)?;
        signal.call(value)?;
        Ok(())
    }
    pub fn get<V: FromLua<'lua>>(&self, key: impl ToLua<'lua>) -> Result<V> {
        let signal: Function = self.1.get(key)?;
        let val: Value = signal.call(())?;
        V::from_lua(val, self.0)
    }
}

impl<T: Renderer> Animation<T> {
    pub fn advance_time(&mut self, time: f32) -> Result<()> {
        // clear canvas
        let width = self.screen.canvas.width() as u32;
        let height = self.screen.canvas.height() as u32;
        self.screen
            .canvas
            .clear_rect(0, 0, width, height, Color::black());

        // draw frame
        self.lua.context(|ctx| {
            let globals = ctx.globals();
            let screen = RefCell::new(&mut self.screen);

            ctx.scope(|scope| {
                // create canvas global
                set_measure(&ctx, scope, |text, _font| {
                    screen
                        .borrow()
                        .canvas
                        .measure_text(0.0, 0.0, text, &Paint::color(Color::white()))
                        .unwrap()
                        .width()
                        * TEXT_SCALE
                })?;

                // create emit function
                let emit = scope.create_function_mut(|ctx, (instr, args): (u8, MultiValue)| {
                    let screen = &mut screen.borrow_mut();
                    let custom = &self.custom;
                    instruction(ctx, instr, args, screen, custom)
                })?;

                // call animation
                let anim: Function = globals.get("$anim")?;
                anim.call((time, emit))
            })
        })?;

        self.screen.canvas.flush();
        Ok(())
    }

    pub fn values(
        &self,
        f: impl for<'lua> Fn(Context<'lua>, AnimationValues<'lua>) -> Result<()>,
    ) -> Result<()> {
        self.lua
            .context(|ctx| f(ctx, AnimationValues(ctx, ctx.globals().get("$value")?)))
    }
}

fn instruction<'lua, T: Renderer>(
    ctx: Context<'lua>,
    instr: u8,
    args: MultiValue<'lua>,
    screen: &mut Screen<T>,
    custom: impl Fn(Context<'lua>, u8, MultiValue<'lua>, &mut Screen<T>) -> Result<()>,
) -> Result<()> {
    match instr {
        0 => {
            let (_, _, a, b, c, d, e, f): (String, bool, f32, f32, f32, f32, f32, f32) =
                FromLuaMulti::from_lua_multi(args, ctx)?;
            screen.push_transform(Mat3::new(a, b, c, d, e, f))
        }
        1 => screen.pop_transform(),
        3 => {
            let width: f32 = FromLuaMulti::from_lua_multi(args, ctx)?;
            screen.line_width = width * screen.root_scale()
        }
        4 => {
            let (x, y, r): (f32, f32, f32) = FromLuaMulti::from_lua_multi(args, ctx)?;
            let middle = screen.point_at(x, y);
            let radius = screen.root_scale() * r;
            screen.draw_circle(middle, radius);
        }
        7 => {
            let (x, y) = FromLuaMulti::from_lua_multi(args, ctx)?;
            let p = screen.point_at(x, y);
            let path = screen.path_start();
            path.move_to(p.x, p.y);
        }
        9 => {
            let (x, y) = FromLuaMulti::from_lua_multi(args, ctx)?;
            let p = screen.point_at(x, y);
            screen.path_op(|path| path.line_to(p.x, p.y));
        }
        10 => screen.path_op(Path::close),
        20 => screen.path_draw(),
        13 => {
            let (x, y, size, text): (f32, f32, f32, String) =
                FromLuaMulti::from_lua_multi(args, ctx)?;
            let rough_scale = screen.rough_scale();
            let font_size = size * TEXT_SCALE * 16.0 * rough_scale;

            screen.canvas.set_transform(&screen.transform().into());
            screen.canvas.scale(1.0 / rough_scale, 1.0 / rough_scale);
            screen
                .canvas
                .fill_text(
                    x,
                    y,
                    text,
                    &Paint::color(Color::white()).with_font_size(font_size),
                )
                .unwrap();
            screen.canvas.reset_transform();
        }
        19 => {
            let (x, y, r): (f32, f32, f32) = FromLuaMulti::from_lua_multi(args, ctx)?;
            let middle = screen.point_at(x, y);
            let vert = screen.point_at(x, y + r) - middle;
            let horz = screen.point_at(x + r, y) - middle;

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
        _ => custom(ctx, instr, args, screen)?,
    };
    Ok(())
}

impl<T: Renderer> Screen<T> {
    pub fn point_at(&self, x: f32, y: f32) -> Vec2 {
        self.transform() * Vec2::new(x, y)
    }
    pub fn rough_scale(&self) -> f32 {
        let trans = self.transform();
        let rough_scale = (Vec2::new(trans.a, trans.b).len_squared().sqrt()
            + Vec2::new(trans.c, trans.d).len_squared().sqrt())
            / 2.0;
        rough_scale
    }
    pub fn root_scale(&self) -> f32 {
        self.transform_stack[0].a
    }
    pub fn transform(&self) -> Mat3 {
        *self.transform_stack.last().unwrap()
    }

    pub fn push_transform(&mut self, mat: Mat3) {
        self.transform_stack.push(self.transform() * mat);
    }
    pub fn pop_transform(&mut self) {
        self.transform_stack.pop();
    }

    pub fn draw_circle(&mut self, center: Vec2, radius: f32) {
        let mut circle = Path::new();
        circle.circle(center.x, center.y, radius);
        self.canvas
            .fill_path(&mut circle, &Paint::color(Color::white()))
    }
    pub fn draw_ellipse(&mut self, focus1: Vec2, focus2: Vec2, sum: f32) {
        if (focus2 - focus1).len_squared() < 1.0 {
            self.draw_circle(focus1, sum / 2.0);
            return;
        } else {
            todo!("{:?} {:?}", focus1, focus2);
        }
    }

    pub fn path_start(&mut self) -> &mut Path {
        self.path.insert(Path::new())
    }
    pub fn path_op(&mut self, op: impl FnOnce(&mut Path)) {
        if let Some(path) = self.path.as_mut() {
            op(path);
        }
    }
    pub fn path_draw(&mut self) {
        if let Some(mut path) = self.path.take() {
            self.canvas.stroke_path(
                &mut path,
                &Paint::color(Color::white()).with_line_width(self.line_width),
            );
        }
    }
}

pub fn animate<T: Renderer + 'static>(
    file: String,
    canvas: Canvas<T>,
    custom: impl for<'lua> Fn(Context<'lua>, u8, MultiValue<'lua>, &mut Screen<T>) -> Result<()>
        + 'static,
    values: impl for<'lua> Fn(Context<'lua>) -> Result<Table<'lua>>,
) -> Result<Animation<T>> {
    let lua = unsafe { Lua::new_with_debug() };

    lua.context(|ctx| {
        // libs
        load_libs(ctx)?;

        let globals = ctx.globals();
        let g_canvas = ctx.create_table()?;

        // canvas signals
        let signal: Table = globals.get("signal")?;
        let signal: Function = signal.get("signal")?;

        let signals = ctx.create_table()?;
        for pair in values(ctx)?.pairs() {
            let (key, value): (Value, Value) = pair?;
            let value: Function = signal.call(value)?;
            signals.set(key, value)?;
        }

        globals.set("$value", signals)?;

        g_canvas.set(
            "signal",
            ctx.create_function(|ctx, name: String| {
                ctx.globals()
                    .get::<_, Table>("$value")?
                    .get::<_, Function>(name)
            })?,
        )?;

        // animation
        globals.set("canvas", g_canvas)?;

        let anim = ctx.scope(|scope| {
            set_measure(&ctx, scope, |text, _font| {
                canvas
                    .measure_text(0.0, 0.0, text, &Paint::color(Color::white()))
                    .unwrap()
                    .width()
                    * TEXT_SCALE
            })?;

            // load animation
            ctx.load(&read_to_string(file).unwrap()).eval::<Function>()
        })?;

        globals.set("$anim", anim)?;
        Ok(())
    })?;

    let width = canvas.width() as usize;
    let height = canvas.height() as usize;

    Ok(Animation {
        lua,
        custom: Box::new(custom),
        screen: Screen {
            canvas,
            transform_stack: vec![Mat3::new(
                width as f32 / 2.0 / 256.0,
                0.0,
                0.0,
                width as f32 / 2.0 / 256.0,
                width as f32 / 2.0,
                height as f32 / 2.0,
            )],
            line_width: 1.0,
            path: None,
        },
    })
}

fn set_measure<'lua, 'scope>(
    ctx: &Context<'lua>,
    scope: &Scope<'lua, 'scope>,
    measure: impl Fn(String, Option<String>) -> f32 + 'scope,
) -> Result<()> {
    let globals = ctx.globals();
    let table: Table = globals.get("canvas")?;
    table.set(
        "measure",
        scope.create_function(move |_, (text, font): (String, Option<String>)| {
            Ok(measure(text, font))
        })?,
    )?;
    Ok(())
}
