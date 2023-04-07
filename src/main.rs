use std::time::Instant;

use femtovg::{renderer::OpenGl, Canvas};
use glutin::{
    config::ConfigTemplateBuilder,
    context::{ContextApi, ContextAttributesBuilder},
    display::GetGlDisplay,
    prelude::{GlDisplay, NotCurrentGlContextSurfaceAccessor},
    surface::{GlSurface, SurfaceAttributesBuilder},
};
use glutin_winit::{DisplayBuilder, GlWindow};
use raw_window_handle::HasRawWindowHandle;
use rlua::Result;
use winit::{
    dpi::PhysicalSize,
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};

mod luanim;

const WIDTH: usize = 1920;
const HEIGHT: usize = 1080;

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

    let mut screen = luanim::animate(
        "luanim/examples/mars.lua".to_owned(),
        canvas,
        |ctx, instr, args, screen| todo!("{}", instr),
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
            // Programs that draw graphics continuously can render here unconditionally for simplicity.
            screen.advance_time(time.elapsed().as_secs_f32()).unwrap();
            surface.swap_buffers(&gl_context).unwrap();
        }
        _ => {}
    });
}
