use anyhow::{Context, Result};
use pixels::{Pixels, SurfaceTexture};
use std::ops::Range;
use std::time::{Duration, Instant};
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop, EventLoopWindowTarget};
use tao::window::{Window, WindowBuilder};

type UpdateFn<State, Surface> = fn(&mut State, &mut Surface) -> Result<()>;
type RenderFn<State, Surface> = fn(&mut State, &mut Surface, Duration) -> Result<()>;
type TaoEventFn<State, Surface> =
    fn(&mut State, &mut Surface, &EventLoopWindowTarget<()>, event: &Event<()>) -> Result<()>;

struct PixelLoop<State, Surface: DrawingSurface> {
    accumulator: Duration,
    current_time: Instant,
    last_time: Instant,
    update_timestep: Duration,
    state: State,
    surface: Surface,
    update: UpdateFn<State, Surface>,
    render: RenderFn<State, Surface>,
}

impl<State, Surface> PixelLoop<State, Surface>
where
    Surface: DrawingSurface,
{
    pub fn new(
        update_fps: usize,
        state: State,
        surface: Surface,
        update: UpdateFn<State, Surface>,
        render: RenderFn<State, Surface>,
    ) -> Self {
        if update_fps == 0 {
            panic!("Designated FPS for updates needs to be > 0");
        }

        Self {
            accumulator: Duration::default(),
            current_time: Instant::now(),
            last_time: Instant::now(),
            update_timestep: Duration::from_nanos(
                (1_000_000_000f64 / update_fps as f64).round() as u64
            ),
            state,
            surface,
            update,
            render,
        }
    }

    // Inpsired by: https://gafferongames.com/post/fix_your_timestep/
    pub fn next_loop(&mut self) -> Result<()> {
        self.last_time = self.current_time;
        self.current_time = Instant::now();
        let mut dt = self.current_time - self.last_time;

        // Escape hatch if update calls take to long in order to not spiral into
        // death
        // @FIXME: It may be useful to make this configurable?
        if dt > Duration::from_millis(100) {
            dt = Duration::from_millis(100);
        }

        while self.accumulator > self.update_timestep {
            (self.update)(&mut self.state, &mut self.surface)?;
            self.accumulator -= self.update_timestep;
        }

        (self.render)(&mut self.state, &mut self.surface, dt)?;

        self.accumulator += dt;
        Ok(())
    }
}

pub fn run<State, Surface: DrawingSurface>(
    state: State,
    surface: Surface,
    update: UpdateFn<State, Surface>,
    render: RenderFn<State, Surface>,
) -> Result<()> {
    let mut pixel_loop = PixelLoop::new(120, state, surface, update, render);
    loop {
        pixel_loop.next_loop().context("run next pixel loop")?;
    }
}

pub struct Color {
    bytes: [u8; 4],
}

impl Color {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let sized_bytes: &[u8; 4] = bytes.try_into().unwrap();
        Self {
            bytes: sized_bytes.clone(),
        }
    }

    pub fn from_rgba(r: u8, b: u8, g: u8, a: u8) -> Self {
        Self {
            bytes: [r, g, b, a],
        }
    }
    pub fn from_rgb(r: u8, b: u8, g: u8) -> Self {
        Self::from_rgba(r, g, b, 255)
    }

    pub fn as_bytes(&self) -> &[u8; 4] {
        &self.bytes
    }
}

pub trait DrawingSurface {
    fn width(&self) -> u32;
    fn height(&self) -> u32;
    fn blit(&mut self) -> Result<()>;
    fn get(&self, x: u32, y: u32) -> Color;
    fn set(&mut self, x: u32, y: u32, color: &Color);
    fn set_range(&mut self, range: Range<usize>, color: &Color);
    fn in_bounds(&self, x: i64, y: i64) -> Option<(u32, u32)>;
    fn physical_pos_to_surface_pos(&self, x: f64, y: f64) -> Option<(u32, u32)>;

    fn clear_screen(&mut self, color: &Color) {
        self.set_range(0..(self.height() * self.width()) as usize, &color);
    }

    fn filled_rect(&mut self, sx: u32, sy: u32, width: u32, height: u32, color: &Color) {
        for y in sy..sy + height {
            self.set_range(
                (y * self.width() + sx) as usize..(y * self.width() + sx + width) as usize,
                color,
            );
        }
    }
}

pub struct PixelsSurface {
    pixels: Pixels,
}

impl PixelsSurface {
    pub fn new(pixels: Pixels) -> Self {
        Self { pixels }
    }
}

impl DrawingSurface for PixelsSurface {
    fn width(&self) -> u32 {
        self.pixels.texture().width()
    }
    fn height(&self) -> u32 {
        self.pixels.texture().height()
    }

    fn blit(&mut self) -> Result<()> {
        self.pixels
            .render()
            .context("letting pixels lib blit to screen")?;
        Ok(())
    }

    fn get(&self, x: u32, y: u32) -> Color {
        let i = ((y * self.width() + x) * 4) as usize;
        let buf = self.pixels.frame();
        Color::from_bytes(&buf[i..i + 4])
    }

    fn set(&mut self, x: u32, y: u32, color: &Color) {
        let i = ((y * self.width() + x) * 4) as usize;
        let buf = self.pixels.frame_mut();
        buf[i..i + 4].copy_from_slice(color.as_bytes());
    }

    fn set_range(&mut self, range: Range<usize>, color: &Color) {
        let byte_range = range.start * 4..range.end * 4;
        let buf = self.pixels.frame_mut();
        for chunk in buf[byte_range].chunks_exact_mut(4) {
            chunk.copy_from_slice(color.as_bytes());
        }
    }

    fn in_bounds(&self, x: i64, y: i64) -> Option<(u32, u32)> {
        if x < 0 || x >= self.width() as i64 || y < 0 || y >= self.height() as i64 {
            None
        } else {
            Some((x as u32, y as u32))
        }
    }

    fn physical_pos_to_surface_pos(&self, x: f64, y: f64) -> Option<(u32, u32)> {
        if let Ok((x, y)) = self.pixels.window_pos_to_pixel((x as f32, y as f32)) {
            Some((x as u32, y as u32))
        } else {
            None
        }
    }
}

pub struct TaoContext {
    event_loop: EventLoop<()>,
    window: Window,
}

impl TaoContext {
    pub fn as_window(&self) -> &Window {
        &self.window
    }
}

pub fn init_tao_window(title: &str, width: u32, height: u32) -> Result<TaoContext> {
    let event_loop = EventLoop::new();
    let window = {
        let size = LogicalSize::new(width, height);
        WindowBuilder::new()
            .with_title(title)
            .with_inner_size(size)
            .with_min_inner_size(size)
            .with_resizable(false)
            .build(&event_loop)?
    };

    Ok(TaoContext { event_loop, window })
}

pub fn init_pixels(context: &TaoContext, width: u32, height: u32) -> Result<PixelsSurface> {
    let physical_dimensions = context.as_window().inner_size();
    let surface_texture = SurfaceTexture::new(
        physical_dimensions.width,
        physical_dimensions.height,
        context.as_window(),
    );
    let pixels = Pixels::new(width, height, surface_texture).context("create pixels surface")?;
    Ok(PixelsSurface::new(pixels))
}

pub fn run_with_tao_and_pixels<State: 'static>(
    state: State,
    context: TaoContext,
    surface: PixelsSurface,
    update: UpdateFn<State, PixelsSurface>,
    render: RenderFn<State, PixelsSurface>,
    handle_event: TaoEventFn<State, PixelsSurface>,
) -> ! {
    let mut pixel_loop = PixelLoop::new(120, state, surface, update, render);
    context.event_loop.run(move |event, window, control_flow| {
        handle_event(
            &mut pixel_loop.state,
            &mut pixel_loop.surface,
            window,
            &event,
        )
        .context("handle user events")
        .unwrap();
        match event {
            Event::MainEventsCleared => {
                pixel_loop
                    .next_loop()
                    .context("run next pixel loop")
                    .unwrap();
            }
            Event::WindowEvent {
                event: win_event, ..
            } => match win_event {
                WindowEvent::CloseRequested => {
                    *control_flow = ControlFlow::Exit;
                }
                _ => {}
            },

            _ => {}
        }
    });
}
