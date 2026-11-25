//! U1-B tour 3 : la scène commune avec la pile modulaire (winit, wgpu,
//! Vello, Parley). Le runtime est minimal et **retained** : la mise en forme
//! des lignes visibles est gardée en cache, et seule la ligne qui change est
//! remise en forme à chaque frame. Virtualisation : 50 lignes visibles de
//! 16 px. Pas de layout Taffy ni d'AccessKit ici (mesurés à part).
use r3_common::{Recorder, Scene};
use std::sync::Arc;
use vello::kurbo::{Affine, Circle, Rect, RoundedRect};
use vello::peniko::{Color, Fill};
use vello::util::{RenderContext, RenderSurface};
use vello::{AaConfig, Renderer, RendererOptions};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

const VISIBLE: usize = 50;

struct Gfx {
    window: Arc<dyn Window>,
    surface: RenderSurface<'static>,
    renderer: Renderer,
}

struct App {
    rec: Recorder,
    scene: Scene,
    cx: RenderContext,
    gfx: Option<Gfx>,
    fcx: parley::FontContext,
    lcx: parley::LayoutContext<()>,
    layouts: Vec<Option<(f32, parley::Layout<()>)>>,
}

impl App {
    fn layout_row(&mut self, i: usize) {
        let v = self.scene.params[i];
        if matches!(&self.layouts[i], Some((old, _)) if *old == v) {
            return;
        }
        let text = self.scene.label(i);
        let mut b = self.lcx.ranged_builder(&mut self.fcx, &text, 1.0, true);
        b.push_default(parley::StyleProperty::FontSize(12.0));
        let mut l = b.build(&text);
        l.break_all_lines(None);
        self.layouts[i] = Some((v, l));
    }

    /// Renders one frame; `false` when no frame was presented (skipped).
    fn draw(&mut self) -> bool {
        self.scene.step();
        for i in 0..VISIBLE.min(self.scene.params.len()) {
            self.layout_row(i);
        }
        let gfx = self.gfx.as_mut().expect("surface");
        let mut sc = vello::Scene::new();
        let (w, h) = (gfx.surface.config.width, gfx.surface.config.height);
        for i in 0..VISIBLE.min(self.scene.params.len()) {
            let y = i as f64 * 16.0;
            let bg = if i % 2 == 0 {
                Color::from_rgb8(36, 39, 45)
            } else {
                Color::from_rgb8(30, 33, 38)
            };
            sc.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                bg,
                None,
                &RoundedRect::new(4.0, y + 1.0, w as f64 - 4.0, y + 15.0, 4.0),
            );
            sc.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                Color::from_rgb8(80, 86, 96),
                None,
                &Rect::new(600.0, y + 6.0, 1200.0, y + 10.0),
            );
            let x = 600.0 + 600.0 * self.scene.params[i] as f64;
            sc.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                Color::from_rgb8(90, 160, 250),
                None,
                &Circle::new((x, y + 8.0), 5.5),
            );
            if let Some((_, l)) = &self.layouts[i] {
                for line in l.lines() {
                    for item in line.items() {
                        if let parley::PositionedLayoutItem::GlyphRun(run) = item {
                            let r = run.run();
                            sc.draw_glyphs(r.font())
                                .font_size(r.font_size())
                                .transform(Affine::translate((8.0, y)))
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
            }
        }
        let dev = &self.cx.devices[gfx.surface.dev_id];
        gfx.renderer
            .render_to_texture(
                &dev.device,
                &dev.queue,
                &sc,
                &gfx.surface.target_view,
                &vello::RenderParams {
                    base_color: Color::from_rgb8(24, 26, 30),
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
                self.cx.configure_surface(&gfx.surface);
                return false;
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return false
            }
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
        true
    }
}

impl ApplicationHandler for App {
    fn can_create_surfaces(&mut self, el: &dyn ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        let attrs = WindowAttributes::default()
            .with_title("r3-modular")
            .with_surface_size(winit::dpi::PhysicalSize::new(1280u32, 800u32));
        let window: Arc<dyn Window> = Arc::from(el.create_window(attrs).expect("window"));
        let size = window.surface_size();
        let surface = pollster::block_on(self.cx.create_surface(
            window.clone(),
            size.width,
            size.height,
            wgpu::PresentMode::AutoVsync,
        ))
        .expect("surface");
        let renderer = Renderer::new(
            &self.cx.devices[surface.dev_id].device,
            RendererOptions::default(),
        )
        .expect("renderer");
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
            WindowEvent::RedrawRequested => {
                if self.draw() && self.rec.tick() {
                    el.exit();
                } else if let Some(g) = &self.gfx {
                    g.window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

fn main() {
    let rec = Recorder::new("modular");
    let scene = Scene::new();
    let n = scene.params.len();
    let app = App {
        rec,
        scene,
        cx: RenderContext::new(),
        gfx: None,
        fcx: parley::FontContext::new(),
        lcx: parley::LayoutContext::new(),
        layouts: (0..n.min(VISIBLE)).map(|_| None).collect(),
    };
    EventLoop::new()
        .expect("event loop")
        .run_app(app)
        .expect("run");
}
