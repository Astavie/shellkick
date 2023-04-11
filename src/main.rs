use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

use fastnes::{
    input::Controllers,
    nes::NES,
    ppu::{Color, FastPPU},
};
use femtovg::{imgref::Img, renderer::OpenGl, rgb::RGBA8, Canvas, ImageFlags, Paint, Path};
use glutin::{
    config::ConfigTemplateBuilder,
    context::{ContextApi, ContextAttributesBuilder},
    display::GetGlDisplay,
    prelude::{GlDisplay, NotCurrentGlContextSurfaceAccessor},
    surface::{GlSurface, SurfaceAttributesBuilder},
};
use glutin_winit::{DisplayBuilder, GlWindow};
use raw_window_handle::HasRawWindowHandle;
use rlua::{FromLuaMulti, Result};
use winit::{
    dpi::PhysicalSize,
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};

mod luanim;

const WIDTH: usize = 1920;
const HEIGHT: usize = 1080;

unsafe fn as_rgba<const N: usize>(p: &[Color; N]) -> &[RGBA8] {
    ::core::slice::from_raw_parts(
        (p as *const [Color; N]) as *const RGBA8,
        ::core::mem::size_of::<[Color; N]>(),
    )
}

fn main() -> Result<()> {
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
    let mut canvas = Canvas::new(opengl).unwrap();

    canvas.set_size(WIDTH as u32, HEIGHT as u32, 1.0);
    canvas.add_font("res/pressstart.ttf").unwrap();

    let mut emulator = NES::read_ines("rom/smb.nes", Controllers::disconnected(), FastPPU::new());

    let frames = Arc::new(Mutex::new(vec![
        [Color {
            r: 127,
            g: 127,
            b: 127,
            a: 255,
        }; 61440],
    ]));

    let frames_animate = frames.clone();

    let mut screen = luanim::animate(
        "script/mario.lua".to_owned(),
        canvas,
        move |ctx, instr, args, screen| match instr {
            // FASTNES
            128 => {
                let (x, y, scale, instance): (f32, f32, f32, usize) =
                    FromLuaMulti::from_lua_multi(args, ctx)?;

                let image = {
                    let frames = frames_animate.lock().unwrap();
                    let frame = &frames[instance - 1];

                    let img = Img::new(unsafe { as_rgba(frame) }, 256, 240);
                    screen
                        .canvas
                        .create_image(img, ImageFlags::NEAREST)
                        .unwrap()
                };

                // divide by 3.75 to make it pixel perfect on full HD screens
                let width = 256.0 / 3.75 * scale;
                let height = 240.0 / 3.75 * scale;

                let fill_paint = Paint::image(image, x, y, width, height, 0.0, 1.0);
                let mut path = Path::new();
                path.rect(x, y, width, height);

                screen.canvas.set_transform(&screen.transform().into());
                screen.canvas.fill_path(&mut path, &fill_paint);
                screen.canvas.reset_transform();

                Ok(())
            }
            _ => todo!("{}", instr),
        },
        |ctx| {
            let values = ctx.create_table()?;
            values.set("frame", 0)?;
            Ok(values)
        },
    )?;

    let time = Instant::now();

    el.run(move |event, _, cf| match event {
        winit::event::Event::WindowEvent {
            ref event,
            window_id,
        } if window_id == window.id() => match event {
            winit::event::WindowEvent::CloseRequested => *cf = ControlFlow::Exit,
            _ => {}
        },
        winit::event::Event::MainEventsCleared => {
            emulator.next_frame();
            frames.lock().unwrap()[0] = emulator.frame();

            screen
                .values(|_ctx, table| {
                    let frame: u32 = table.get("frame")?;
                    table.set("frame", frame + 1)?;
                    Ok(())
                })
                .unwrap();

            // Programs that draw graphics continuously can render here unconditionally for simplicity.
            screen.advance_time(time.elapsed().as_secs_f32()).unwrap();
            surface.swap_buffers(&gl_context).unwrap();
        }
        _ => {}
    });
}
