use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicU8, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::Instant,
};

use fastnes::{
    input::Controllers,
    nes::NES,
    ppu::{Color, DrawOptions, FastPPU},
};
use femtovg::{imgref::Img, renderer::OpenGl, rgb::RGBA8, Canvas, ImageFlags, Paint, Path};
use glutin::{
    config::{Config, ConfigTemplateBuilder},
    context::{ContextApi, ContextAttributesBuilder},
    display::GetGlDisplay,
    prelude::{GlDisplay, NotCurrentGlContextSurfaceAccessor},
    surface::{GlSurface, SurfaceAttributesBuilder},
};
use glutin_winit::{DisplayBuilder, GlWindow};
use luanim::Animation;
use notify::{Event, EventKind, RecursiveMode, Watcher};
use rand::Rng;
use raw_window_handle::HasRawWindowHandle;
use rlua::{FromLuaMulti, Result, Table};
use spin_sleep::LoopHelper;
use threadpool::ThreadPool;
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

#[derive(Clone, Debug)]
struct Personality {
    patient: u32, // stuck iterations before random movement
    random: u32,  // frames per random movement
    playful: u32, // frames per regular movement
    twitchy: f32, // likelyhood of button switch per frame
    smart: u32,   // amount of paths to try
}

struct Mario {
    personality: Personality,
    revert_stack: Vec<u32>,
    being_random: Option<u32>,

    stuck_count: u32,
    inputs_past: Vec<u8>,
    inputs_future: VecDeque<u8>,

    states: Vec<NES>,
}

fn next_input(prev: u8, personality: &Personality) -> u8 {
    let mut rng = rand::thread_rng();
    let mut next = prev;
    while rng.gen_range(0.0..1.0) < personality.twitchy {
        let mut flip = 1 << rng.gen_range(0..6);
        if flip > 0b00000011 {
            // skip start and select
            flip = flip << 2
        }
        next ^= flip;
    }
    next
}

#[derive(PartialEq)]
enum Fitness {
    Dying(bool),
    Cutscene,
    Level(u64),
}

impl PartialOrd for Fitness {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Fitness::Dying(_), Fitness::Dying(_)) => Some(std::cmp::Ordering::Equal),
            (Fitness::Dying(_), Fitness::Cutscene) => Some(std::cmp::Ordering::Less),
            (Fitness::Dying(_), Fitness::Level(_)) => Some(std::cmp::Ordering::Less),
            (Fitness::Cutscene, Fitness::Dying(_)) => Some(std::cmp::Ordering::Greater),
            (Fitness::Cutscene, Fitness::Cutscene) => Some(std::cmp::Ordering::Equal),
            (Fitness::Cutscene, Fitness::Level(_)) => Some(std::cmp::Ordering::Greater),
            (Fitness::Level(_), Fitness::Dying(_)) => Some(std::cmp::Ordering::Greater),
            (Fitness::Level(_), Fitness::Cutscene) => Some(std::cmp::Ordering::Less),
            (Fitness::Level(a), Fitness::Level(b)) => u64::partial_cmp(a, b),
        }
    }
}

fn fitness(nes: &mut NES) -> Fitness {
    let level_pos = u16::from(nes.read(0x6d)) << 8 // screen page
                    | u16::from(nes.read(0x86)); // screen x

    let mario_position: u32 = u32::from(nes.read(0x075f)) << 24
        | u32::from(nes.read(0x0760)) << 16
        | u32::from(level_pos);

    let engine = nes.read(0x0e);
    let task = nes.read(0x0772);
    let mode = nes.read(0x0770);

    let mario_y = u16::from(nes.read(0xb5)) << 8 | u16::from(nes.read(0xce));
    let cutscene = engine <= 5 || engine == 7 || mode == 2 || (mode == 1 && task != 3);
    let dying =
        (mario_y > 456 || engine == 6 || engine == 11 || mode == 0 || mode == 3) && !cutscene;

    let time = u16::from(nes.read(0x07f8)) * 100
        + u16::from(nes.read(0x07f9)) * 10
        + u16::from(nes.read(0x07fa));

    let out_of_time = time == 0 && !cutscene;

    if dying || out_of_time {
        Fitness::Dying(out_of_time)
    } else if cutscene {
        Fitness::Cutscene
    } else {
        Fitness::Level(u64::from(mario_position))
    }
}

fn scroll(nes: &mut NES) -> u32 {
    let level_pos = u16::from(nes.read(0x071a)) << 8 // screen page
                    | u16::from(nes.read(0x071c)); // screen x

    let mario_position: u32 = u32::from(nes.read(0x075f)) << 24
        | u32::from(nes.read(0x0760)) << 16
        | u32::from(level_pos);

    mario_position
}

fn next_frame(mario: &mut Mario) {
    let input = Arc::new(AtomicU8::new(0));
    let mut nes = mario.states.pop().unwrap();
    nes.set_controllers(Controllers::standard(&input));
    let mut score = fitness(&mut nes);

    // get new inputs
    if mario.inputs_future.is_empty() {
        mario.states.push(nes.clone());

        if score == Fitness::Dying(false) || score == Fitness::Dying(true) {
            // do revert
            revert(
                mario,
                if score == Fitness::Dying(true) {
                    360 * 20 / mario.personality.playful
                } else {
                    2
                },
            );
            nes = mario.states.pop().unwrap();
            nes.set_controllers(Controllers::standard(&input));
            score = fitness(&mut nes);
        }

        if let Some(num) = mario.being_random.as_mut() {
            // Random input
            *num -= 1;
            if *num == 0 {
                mario.being_random = None;
            }

            let mut last = *mario.inputs_past.last().unwrap();
            for _ in 0..mario.personality.playful {
                last = next_input(last, &mario.personality);
                mario.inputs_future.push_back(last);
            }
        } else {
            // Regular input
            let mut best_result = Fitness::Dying(false);

            for i in 0..mario.personality.smart {
                // generate inputs
                let mut list = VecDeque::new();
                let mut last = *mario.inputs_past.last().unwrap();
                for _ in 0..mario.personality.playful {
                    if i > 0 {
                        last = next_input(last, &mario.personality);
                    }
                    list.push_back(last);
                }

                // run
                let mut cloned = nes.clone();
                let input = Arc::new(AtomicU8::new(0));
                cloned.set_controllers(Controllers::standard(&input));

                for item in list.iter().copied() {
                    input.store(item, Ordering::Relaxed);
                    cloned.next_frame();
                }

                // get results
                let score = fitness(&mut cloned);
                if score >= best_result {
                    best_result = score;
                    mario.inputs_future = list;
                }
            }

            // test against current score
            if best_result <= score && best_result != Fitness::Cutscene {
                mario.stuck_count += 1;
                if mario.stuck_count >= mario.personality.patient {
                    mario.stuck_count = 0;
                    mario.being_random = Some(mario.personality.random);
                }
            }
        }

        mario.revert_stack.push(0);
    }

    // set input
    let item = mario.inputs_future.pop_front().unwrap();
    mario.inputs_past.push(item);
    input.store(item, Ordering::Relaxed);

    // next frame
    nes.next_frame();

    // push nes back in
    mario.states.push(nes);
}

fn revert(mario: &mut Mario, amount: u32) {
    // Revert iteration
    for _ in 0..amount {
        mario.revert_stack.pop();
        mario.states.pop();
        for _ in 0..mario.personality.playful {
            mario.inputs_past.pop();
        }
    }

    while *mario.revert_stack.last().unwrap() >= 1 {
        mario.revert_stack.pop();
        mario.states.pop();
        for _ in 0..mario.personality.playful {
            mario.inputs_past.pop();
        }
    }

    *mario.revert_stack.last_mut().unwrap() += 1;
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

    let mut marios = VecDeque::new();
    let mut backgrounds = Vec::new();
    let mut sprites = Vec::new();
    let mut scores = Vec::new();

    let mut rng = rand::thread_rng();
    for _ in 0..64 {
        let mut mario = Mario {
            personality: Personality {
                patient: rng.gen_range(10..30),   // 3,
                random: rng.gen_range(1..10),     // 10,
                playful: rng.gen_range(8..12),    // 20,
                twitchy: rng.gen_range(0.2..0.9), // 0.5,
                smart: 2,                         // 4,
            },
            being_random: None,
            revert_stack: vec![0],
            stuck_count: 0,
            inputs_past: vec![],
            inputs_future: vec![
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0b00001000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ]
            .into(),
            states: vec![NES::read_ines(
                "rom/smb.nes",
                Controllers::disconnected(),
                FastPPU::new(),
            )],
        };
        for _ in 0..rng.gen_range(0..20) {
            mario.inputs_future.push_back(0)
        }
        marios.push_back(mario);

        backgrounds.push(
            [Color {
                r: 127,
                g: 127,
                b: 127,
                a: 255,
            }; 61440],
        );
        sprites.push(
            [Color {
                r: 127,
                g: 127,
                b: 127,
                a: 0,
            }; 61440],
        );
        scores.push(0u32);
    }
    let backgrounds = Arc::new(Mutex::new(backgrounds));
    let sprites = Arc::new(Mutex::new(sprites));
    let scores = Arc::new(Mutex::new(scores));
    let personalities: Vec<Personality> = marios
        .iter()
        .map(|mario| mario.personality.clone())
        .collect();

    let scores_clone = scores.clone();
    let bg_clone = backgrounds.clone();
    let spr_clone = sprites.clone();
    thread::spawn(move || {
        let count = 12;
        let pool = ThreadPool::new(count);

        let mut subdivides = Vec::new();
        let amount = marios.len();
        for i in 0..count {
            let mut submarios = Vec::new();
            for j in 0..(amount - 1) / count + 1 {
                if marios.is_empty() {
                    break;
                }
                submarios.push((
                    i * ((amount - 1) / count + 1) + j,
                    marios.pop_front().unwrap(),
                ))
            }
            subdivides.push(submarios);
        }

        while let Some(mut submarios) = subdivides.pop() {
            let scores_clone = scores_clone.clone();
            let bg_clone = bg_clone.clone();
            let spr_clone = spr_clone.clone();
            pool.execute(move || {
                let mut loop_helper = LoopHelper::builder().build_with_target_rate(60.0);

                loop {
                    println!("{:?}", loop_helper.loop_start());
                    for (i, mario) in submarios.iter_mut() {
                        next_frame(mario);

                        let nes = mario.states.last_mut().unwrap();

                        scores_clone.lock().unwrap()[*i] = scroll(nes);
                        bg_clone.lock().unwrap()[*i] = nes.frame(DrawOptions::Background);
                        spr_clone.lock().unwrap()[*i] = nes.frame(DrawOptions::Sprites);
                    }
                    loop_helper.loop_sleep();
                }
            });
        }
    });

    let mut screen = animate(
        "script/mario.lua",
        config.clone(),
        backgrounds.clone(),
        sprites.clone(),
        &personalities,
    )?;

    let (tx_event, rx_event) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(tx_event).unwrap();
    watcher
        .watch(::std::path::Path::new("script"), RecursiveMode::Recursive)
        .unwrap();

    let mut time = Instant::now();

    el.run(move |event, _, cf| match event {
        winit::event::Event::WindowEvent {
            ref event,
            window_id,
        } if window_id == window.id() => match event {
            winit::event::WindowEvent::CloseRequested => *cf = ControlFlow::Exit,
            _ => {}
        },
        winit::event::Event::MainEventsCleared => {
            let mut refresh = false;
            while let Ok(event) = rx_event.try_recv() {
                match event {
                    Ok(Event {
                        kind: EventKind::Modify(_),
                        ..
                    }) => refresh = true,
                    Ok(_) => {}
                    Err(e) => println!("watch error: {:?}", e),
                }
            }
            if refresh {
                // refresh screen
                match animate(
                    "script/mario.lua",
                    config.clone(),
                    backgrounds.clone(),
                    sprites.clone(),
                    &personalities,
                ) {
                    Ok(s) => {
                        screen = s;
                        time = Instant::now();
                    }
                    Err(e) => println!("lua error: {}", e),
                }
            }

            screen
                .values(|_ctx, table| {
                    let frame: u32 = table.get("frame")?;
                    table.set("frame", frame + 1)?;

                    let marios: Table = table.get("marios")?;
                    for (i, result) in scores.lock().unwrap().iter().enumerate() {
                        let index = i + 1;
                        let mario: Table = marios.get(index)?;
                        mario.set("fitness", *result)?;
                    }
                    table.set("marios", marios)?;
                    Ok(())
                })
                .unwrap();

            // Programs that draw graphics continuously can render here unconditionally for simplicity.
            screen
                .advance_time(time.elapsed().as_secs_f32())
                .unwrap_or_else(|e| println!("lua error: {}", e));
            surface.swap_buffers(&gl_context).unwrap();
        }
        _ => {}
    });
}

fn animate(
    path: &str,
    config: Config,
    backgrounds: Arc<Mutex<Vec<[Color; 61440]>>>,
    sprites: Arc<Mutex<Vec<[Color; 61440]>>>,
    marios: &Vec<Personality>,
) -> Result<Animation<OpenGl>> {
    let opengl = OpenGl::new_from_glutin_display(&config.display()).unwrap();
    let mut canvas = Canvas::new(opengl).unwrap();
    canvas.set_size(WIDTH as u32, HEIGHT as u32, 1.0);
    canvas.add_font("res/pressstart.ttf").unwrap();

    luanim::animate(
        path.to_owned(),
        canvas,
        move |ctx, instr, args, screen| match instr {
            // FASTNES
            128 => {
                let (x, y, scale, instance): (f32, f32, f32, usize) =
                    FromLuaMulti::from_lua_multi(args, ctx)?;

                let image = {
                    let frames = backgrounds.lock().unwrap();
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

                // need to flush the canvas before being able to delete the image
                screen.canvas.flush();
                screen.canvas.delete_image(image);
                Ok(())
            }
            129 => {
                let (x, y, scale, instance, xo, yo, opacity): (
                    f32,
                    f32,
                    f32,
                    usize,
                    f32,
                    f32,
                    f32,
                ) = FromLuaMulti::from_lua_multi(args, ctx)?;

                let image = {
                    let frames = sprites.lock().unwrap();
                    let frame = &frames[instance - 1];

                    let img = Img::new(unsafe { as_rgba(frame) }, 256, 240);
                    screen
                        .canvas
                        .create_image(img, ImageFlags::NEAREST)
                        .unwrap()
                };

                // divide by 3.75 to make it pixel perfect on full HD screens
                let pixel = 1.0 / 3.75 * scale;
                let width = 256.0 * pixel;
                let height = 240.0 * pixel;

                let fill_paint = Paint::image(
                    image,
                    x + xo * pixel,
                    y + yo * pixel,
                    width,
                    height,
                    0.0,
                    opacity,
                );
                let mut path = Path::new();
                path.rect(
                    f32::max(x, x + xo * pixel),
                    f32::max(x, x + yo * pixel),
                    f32::min(width - xo * pixel, width + xo * pixel),
                    f32::min(height - yo * pixel, height + yo * pixel),
                );

                screen.canvas.set_transform(&screen.transform().into());
                screen.canvas.fill_path(&mut path, &fill_paint);
                screen.canvas.reset_transform();

                // need to flush the canvas before being able to delete the image
                screen.canvas.flush();
                screen.canvas.delete_image(image);
                Ok(())
            }
            _ => todo!("{}", instr),
        },
        |ctx| {
            let values = ctx.create_table()?;
            values.set("frame", 0)?;

            let marios_data = ctx.create_table()?;
            for (i, mario) in marios.iter().enumerate() {
                let personality = ctx.create_table()?;
                personality.set("patient", mario.patient)?;
                personality.set("random", mario.random)?;
                personality.set("playful", mario.playful)?;
                personality.set("twitchy", mario.twitchy)?;
                personality.set("smart", mario.smart)?;

                let data = ctx.create_table()?;
                data.set("personality", personality)?;
                data.set("fitness", 0)?;

                let index = i + 1;
                marios_data.set(index, data)?;
            }
            values.set("marios", marios_data)?;

            Ok(values)
        },
    )
}
