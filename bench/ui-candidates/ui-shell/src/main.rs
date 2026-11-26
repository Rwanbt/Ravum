//! U2 : l'application de qualification 0 dans une vraie fenêtre, sur la pile
//! modulaire (UI-D21) : winit (fenêtre, entrées), wgpu et Vello (rendu),
//! Parley (texte). `ui-core` garde l'arbre, le layout, le focus, le
//! défilement et la logique ; cette coquille ne fait que traduire les
//! entrées et exécuter les ordres de [`ui_shell::paint`].
//!
//! Démarrage repris du banc (BENCH-U1B-ROUND3, première frame) : le GPU, les
//! pipelines et les polices se préparent dans un thread pendant que la
//! fenêtre se crée ; instance wgpu sur l'API native seule.
//!
//! `UI_SHELL_EXIT_FRAMES=n` : quitte après `n` frames (essai automatique).

use std::collections::HashMap;
use std::sync::Arc;
use ui_core::Event;
use ui_shell::{paint, Op, Shell};
use vello::kurbo::{Affine, Rect as KRect, RoundedRect, Stroke};
use vello::peniko::{Color, Fill};
use vello::util::{DeviceHandle, RenderContext, RenderSurface};
use vello::{AaConfig, AaSupport, Renderer, RendererOptions};
use winit::application::ApplicationHandler;
use winit::event::{ButtonSource, ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowAttributes, WindowId};

const FONT_SIZE: f32 = 13.0;

struct Warm {
    cx: RenderContext,
    dev_id: usize,
    renderer: Renderer,
    fcx: parley::FontContext,
}

fn render_context() -> RenderContext {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        display: None,
        backends: wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY),
        flags: wgpu::InstanceFlags::from_build_config().with_env(),
        memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
        backend_options: wgpu::BackendOptions::from_env_or_default(),
    });
    RenderContext {
        instance,
        devices: Vec::new(),
    }
}

fn make_renderer(dev: &DeviceHandle) -> Renderer {
    Renderer::new(
        &dev.device,
        RendererOptions {
            antialiasing_support: AaSupport::area_only(),
            ..RendererOptions::default()
        },
    )
    .expect("renderer")
}

fn warm() -> Warm {
    let fonts = std::thread::spawn(|| {
        let mut fcx = parley::FontContext::new();
        let mut lcx = parley::LayoutContext::<()>::new();
        let mut b = lcx.ranged_builder(&mut fcx, "compteur", 1.0, true);
        b.push_default(parley::StyleProperty::FontSize(FONT_SIZE));
        b.build("compteur").break_all_lines(None);
        fcx
    });
    let mut cx = render_context();
    let dev_id = pollster::block_on(cx.device(None)).expect("no GPU adapter");
    let renderer = make_renderer(&cx.devices[dev_id]);
    Warm {
        cx,
        dev_id,
        renderer,
        fcx: fonts.join().expect("font thread"),
    }
}

struct Gfx {
    window: Arc<dyn Window>,
    surface: RenderSurface<'static>,
    renderer: Renderer,
}

struct App {
    shell: Shell,
    warm: Option<std::thread::JoinHandle<Warm>>,
    cx: Option<RenderContext>,
    fcx: Option<parley::FontContext>,
    lcx: parley::LayoutContext<()>,
    /// Mise en forme gardée par texte (retained) ; vidée si elle grossit.
    layouts: HashMap<(String, u32), parley::Layout<()>>,
    gfx: Option<Gfx>,
    pointer: (f64, f64),
    frames: u64,
    exit_after: Option<u64>,
}

fn shade(level: usize) -> Color {
    let v = 28 + 8 * (level.min(4) as u8);
    Color::from_rgb8(v, v + 3, v + 8)
}

impl App {
    fn scale(&self) -> f64 {
        self.gfx.as_ref().map_or(1.0, |g| g.window.scale_factor())
    }

    fn layout(&mut self, text: &str, scale: f32) -> &parley::Layout<()> {
        let k = (text.to_string(), scale.to_bits());
        if !self.layouts.contains_key(&k) {
            if self.layouts.len() > 4096 {
                self.layouts.clear();
            }
            let fcx = self.fcx.as_mut().expect("fonts");
            let mut b = self.lcx.ranged_builder(fcx, text, scale, true);
            b.push_default(parley::StyleProperty::FontSize(FONT_SIZE));
            let mut l = b.build(text);
            l.break_all_lines(None);
            self.layouts.insert(k.clone(), l);
        }
        &self.layouts[&k]
    }

    fn draw(&mut self) {
        let s = self.scale();
        let ops = paint(&self.shell.ui);
        let mut sc = vello::Scene::new();
        let px = |r: &ui_core::Rect| {
            KRect::new(
                r.x as f64 * s,
                r.y as f64 * s,
                (r.x + r.w) as f64 * s,
                (r.y + r.h) as f64 * s,
            )
        };
        for op in &ops {
            match op {
                Op::Fill { rect, level } => sc.fill(
                    Fill::NonZero,
                    Affine::IDENTITY,
                    shade(*level),
                    None,
                    &RoundedRect::from_rect(px(rect), 4.0 * s),
                ),
                Op::Focus { rect } => sc.stroke(
                    &Stroke::new(2.0 * s),
                    Affine::IDENTITY,
                    Color::from_rgb8(90, 160, 250),
                    None,
                    &RoundedRect::from_rect(px(rect).inset(-s), 4.0 * s),
                ),
                Op::Text { at, clip, text } => {
                    let (x, y) = (at.0 as f64 * s, at.1 as f64 * s);
                    let clip = px(clip);
                    let l = self.layout(text, s as f32);
                    sc.push_clip_layer(Fill::NonZero, Affine::IDENTITY, &clip);
                    for line in l.lines() {
                        for item in line.items() {
                            if let parley::PositionedLayoutItem::GlyphRun(run) = item {
                                let r = run.run();
                                sc.draw_glyphs(r.font())
                                    .font_size(r.font_size())
                                    .transform(Affine::translate((x, y)))
                                    .brush(Color::from_rgb8(230, 230, 230))
                                    .draw(
                                        Fill::NonZero,
                                        run.positioned_glyphs().map(|g| vello::Glyph {
                                            id: g.id,
                                            x: g.x,
                                            y: g.y,
                                        }),
                                    );
                            }
                        }
                    }
                    sc.pop_layer();
                }
            }
        }
        let cx = self.cx.as_ref().expect("gpu");
        let gfx = self.gfx.as_mut().expect("surface");
        let dev = &cx.devices[gfx.surface.dev_id];
        let (w, h) = (gfx.surface.config.width, gfx.surface.config.height);
        gfx.renderer
            .render_to_texture(
                &dev.device,
                &dev.queue,
                &sc,
                &gfx.surface.target_view,
                &vello::RenderParams {
                    base_color: Color::from_rgb8(20, 22, 26),
                    width: w,
                    height: h,
                    antialiasing_method: AaConfig::Area,
                },
            )
            .expect("render");
        let frame = match gfx.surface.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated => {
                cx.configure_surface(&gfx.surface);
                gfx.window.request_redraw();
                return;
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => return,
            other => panic!("surface lost: {other:?}"),
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut enc = dev
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        gfx.surface
            .blitter
            .copy(&dev.device, &mut enc, &gfx.surface.target_view, &view);
        dev.queue.submit([enc.finish()]);
        dev.queue.present(frame);
        self.frames += 1;
    }

    fn input(&mut self, e: Event) {
        if self.shell.event(&e) {
            if let Some(g) = &self.gfx {
                g.window.request_redraw();
            }
        }
    }
}

impl ApplicationHandler for App {
    fn can_create_surfaces(&mut self, el: &dyn ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        let attrs = WindowAttributes::default()
            .with_title("Ravum UI — application 0")
            .with_surface_size(winit::dpi::LogicalSize::new(1024.0, 700.0));
        let window: Arc<dyn Window> = Arc::from(el.create_window(attrs).expect("window"));
        let size = window.surface_size();
        let Warm {
            cx,
            dev_id,
            renderer,
            fcx,
        } = self
            .warm
            .take()
            .expect("warm-up")
            .join()
            .expect("warm-up thread");
        let cx = self.cx.insert(cx);
        self.fcx = Some(fcx);
        let surface = pollster::block_on(cx.create_render_surface(
            cx.instance.create_surface(window.clone()).expect("surface"),
            size.width,
            size.height,
            wgpu::PresentMode::AutoVsync,
        ))
        .expect("surface");
        // Fail-closed: a renderer made for a device that cannot present to
        // this window is rebuilt for the one that can.
        let renderer = if surface.dev_id == dev_id {
            renderer
        } else {
            make_renderer(&cx.devices[surface.dev_id])
        };
        let s = window.scale_factor();
        self.shell
            .resize((size.width as f32 / s as f32, size.height as f32 / s as f32));
        window.request_redraw();
        self.gfx = Some(Gfx {
            window,
            surface,
            renderer,
        });
    }

    fn window_event(&mut self, el: &dyn ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::SurfaceResized(size) if size.width > 0 && size.height > 0 => {
                let s = self.scale();
                if let (Some(cx), Some(g)) = (&self.cx, &mut self.gfx) {
                    cx.resize_surface(&mut g.surface, size.width, size.height);
                    g.window.request_redraw();
                }
                self.shell
                    .resize((size.width as f32 / s as f32, size.height as f32 / s as f32));
            }
            WindowEvent::PointerMoved { position, .. } => {
                self.pointer = (position.x, position.y);
            }
            WindowEvent::PointerButton {
                state: ElementState::Pressed,
                position,
                primary: true,
                button: ButtonSource::Mouse(MouseButton::Left) | ButtonSource::Touch { .. },
                ..
            } => {
                let s = self.scale();
                self.pointer = (position.x, position.y);
                self.input(Event::Click {
                    x: (position.x / s) as f32,
                    y: (position.y / s) as f32,
                });
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let s = self.scale();
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -y * 48.0,
                    MouseScrollDelta::PixelDelta(p) => (-p.y / s) as f32,
                    // Unknown future units: ignored rather than guessed.
                    _ => return,
                };
                let (x, y) = ((self.pointer.0 / s) as f32, (self.pointer.1 / s) as f32);
                self.input(Event::Scroll { x, y, dy });
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed
                    && event.logical_key == Key::Named(NamedKey::Tab) =>
            {
                self.input(Event::FocusNext);
            }
            WindowEvent::RedrawRequested => {
                self.draw();
                if self.exit_after.is_some_and(|n| self.frames >= n) {
                    el.exit();
                } else if self.exit_after.is_some() {
                    if let Some(g) = &self.gfx {
                        g.window.request_redraw();
                    }
                }
            }
            _ => {}
        }
    }
}

fn main() {
    let warm = std::thread::spawn(warm);
    let app = App {
        shell: Shell::new((1024.0, 700.0)),
        warm: Some(warm),
        cx: None,
        fcx: None,
        lcx: parley::LayoutContext::new(),
        layouts: HashMap::new(),
        gfx: None,
        pointer: (0.0, 0.0),
        frames: 0,
        exit_after: std::env::var("UI_SHELL_EXIT_FRAMES")
            .ok()
            .and_then(|v| v.parse().ok()),
    };
    EventLoop::new()
        .expect("event loop")
        .run_app(app)
        .expect("run");
}
