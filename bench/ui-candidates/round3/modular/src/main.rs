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

static T0: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// `R3_PHASES=1`: elapsed time since `main` at each startup phase, on stderr.
fn phase(name: &str) {
    if std::env::var_os("R3_PHASES").is_some() {
        let ms = T0.get().map_or(0.0, |t| t.elapsed().as_secs_f64() * 1e3);
        eprintln!("phase {name} {ms:.1}");
    }
}

struct Gfx {
    window: Arc<dyn Window>,
    surface: RenderSurface<'static>,
    renderer: Renderer,
    cache: Option<PipelineCacheFile>,
}

/// Driver pipeline cache kept on disk between runs (`R3_NO_PIPELINE_CACHE`
/// turns it off). Only where wgpu supports it (Vulkan); elsewhere `None`.
struct PipelineCacheFile {
    cache: wgpu::PipelineCache,
    path: std::path::PathBuf,
}

impl PipelineCacheFile {
    fn open(adapter: &wgpu::Adapter, device: &wgpu::Device) -> Option<Self> {
        if std::env::var_os("R3_NO_PIPELINE_CACHE").is_some()
            || !device.features().contains(wgpu::Features::PIPELINE_CACHE)
        {
            return None;
        }
        let key = wgpu::util::pipeline_cache_key(&adapter.get_info())?;
        let dir = std::env::var_os("XDG_CACHE_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".cache")))?
            .join("ravum-ui-bench");
        let path = dir.join(key);
        let data = std::fs::read(&path).ok();
        // SAFETY: wgpu requires the data to come from `PipelineCache::get_data`
        // on a compatible adapter. The file is only written by `save` below,
        // under a name that encodes the adapter and driver
        // (`pipeline_cache_key`), and `fallback: true` makes the driver
        // discard data it rejects instead of failing.
        let cache = unsafe {
            device.create_pipeline_cache(&wgpu::PipelineCacheDescriptor {
                label: Some("vello"),
                data: data.as_deref(),
                fallback: true,
            })
        };
        Some(Self { cache, path })
    }

    /// Written after the first frame, off the critical path; atomic rename.
    fn save(&self) {
        let Some(data) = self.cache.get_data() else {
            return;
        };
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = self.path.with_extension("tmp");
        if std::fs::write(&tmp, data).is_ok() {
            let _ = std::fs::rename(&tmp, &self.path);
        }
    }
}

struct App {
    rec: Recorder,
    scene: Scene,
    cx: Option<RenderContext>,
    warm: Option<std::thread::JoinHandle<Warm>>,
    gfx: Option<Gfx>,
    fcx: Option<parley::FontContext>,
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
        let mut b = self
            .lcx
            .ranged_builder(self.fcx.as_mut().expect("fonts"), &text, 1.0, true);
        b.push_default(parley::StyleProperty::FontSize(12.0));
        let mut l = b.build(&text);
        l.break_all_lines(None);
        self.layouts[i] = Some((v, l));
    }

    /// Renders one frame; `false` when no frame was presented (skipped).
    fn draw(&mut self) -> bool {
        self.scene.step();
        let first = self.layouts.iter().all(Option::is_none);
        for i in 0..VISIBLE.min(self.scene.params.len()) {
            self.layout_row(i);
        }
        if first {
            phase("text_layout");
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
        let cx = self.cx.as_ref().expect("gpu");
        let dev = &cx.devices[gfx.surface.dev_id];
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
                cx.configure_surface(&gfx.surface);
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
        if first {
            phase("first_present");
            if let Some(c) = &gfx.cache {
                c.save();
            }
        }
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
        phase("event_loop_ready");
        let window: Arc<dyn Window> = Arc::from(el.create_window(attrs).expect("window"));
        phase("window");
        let size = window.surface_size();
        // GPU and fonts were prepared while the window was being created
        // (`warm`); without `R3_SERIAL_INIT`, this only joins the thread.
        let Warm {
            cx,
            dev_id,
            renderer,
            cache,
            fcx,
        } = self
            .warm
            .take()
            .expect("warm-up")
            .join()
            .expect("warm-up thread");
        let cx = self.cx.insert(cx);
        self.fcx = Some(fcx);
        phase("warm_joined");
        let surface = pollster::block_on(cx.create_render_surface(
            cx.instance.create_surface(window.clone()).expect("surface"),
            size.width,
            size.height,
            wgpu::PresentMode::AutoVsync,
        ))
        .expect("surface");
        phase("surface");
        // Fail-closed: a renderer made for another device than the one that
        // can present to this window is rebuilt for the right one.
        let (renderer, cache) = if surface.dev_id == dev_id {
            (renderer, cache)
        } else {
            make_renderer(&cx.devices[surface.dev_id])
        };
        phase("vello_pipelines");
        window.request_redraw();
        self.gfx = Some(Gfx {
            window,
            surface,
            renderer,
            cache,
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

/// Everything the first frame needs that does not need the window: wgpu
/// instance, adapter and device, Vello pipelines, system font collection.
struct Warm {
    cx: RenderContext,
    dev_id: usize,
    renderer: Renderer,
    cache: Option<PipelineCacheFile>,
    fcx: parley::FontContext,
}

fn make_renderer(dev: &vello::util::DeviceHandle) -> (Renderer, Option<PipelineCacheFile>) {
    let cache = PipelineCacheFile::open(dev.adapter(), &dev.device);
    let renderer = Renderer::new(
        &dev.device,
        RendererOptions {
            // Only `AaConfig::Area` is drawn.
            antialiasing_support: if std::env::var_os("R3_AA_ALL").is_some() {
                vello::AaSupport::all()
            } else {
                vello::AaSupport::area_only()
            },
            pipeline_cache: cache.as_ref().map(|c| c.cache.clone()),
            ..RendererOptions::default()
        },
    )
    .expect("renderer");
    (renderer, cache)
}

fn load_fonts() -> parley::FontContext {
    let mut fcx = parley::FontContext::new();
    // Loads the system font collection now rather than at the first layout.
    let mut lcx = parley::LayoutContext::<()>::new();
    let mut b = lcx.ranged_builder(&mut fcx, "param 0: 0.000", 1.0, true);
    b.push_default(parley::StyleProperty::FontSize(12.0));
    b.build("param 0: 0.000").break_all_lines(None);
    phase("w_fonts");
    fcx
}

fn warm() -> Warm {
    // The system font collection loads on its own thread, beside the GPU.
    let fonts = std::thread::spawn(load_fonts);
    let mut cx = RenderContext::new();
    phase("w_instance");
    let dev_id = pollster::block_on(cx.device(None)).expect("no GPU adapter");
    phase("w_device");
    let (mut renderer, cache) = make_renderer(&cx.devices[dev_id]);
    phase("w_pipelines");
    // One throw-away render: drivers that compile pipelines at first use
    // (Mesa) do it here, off the critical path. `R3_NO_PREWARM` skips it.
    if std::env::var_os("R3_NO_PREWARM").is_none() {
        let dev = &cx.devices[dev_id];
        let tex = dev.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("prewarm"),
            size: wgpu::Extent3d {
                width: 64,
                height: 64,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
            format: wgpu::TextureFormat::Rgba8Unorm,
            view_formats: &[],
        });
        let mut sc = vello::Scene::new();
        sc.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            Color::from_rgb8(1, 2, 3),
            None,
            &Circle::new((32.0, 32.0), 8.0),
        );
        renderer
            .render_to_texture(
                &dev.device,
                &dev.queue,
                &sc,
                &tex.create_view(&wgpu::TextureViewDescriptor::default()),
                &vello::RenderParams {
                    base_color: Color::from_rgb8(0, 0, 0),
                    width: 64,
                    height: 64,
                    antialiasing_method: AaConfig::Area,
                },
            )
            .expect("prewarm render");
        let _ = dev.device.poll(wgpu::PollType::wait_indefinitely());
        phase("w_prewarm");
    }
    let fcx = fonts.join().expect("font thread");
    Warm {
        cx,
        dev_id,
        renderer,
        cache,
        fcx,
    }
}

fn main() {
    let rec = Recorder::new("modular");
    T0.get_or_init(std::time::Instant::now);
    // `R3_SERIAL_INIT`: the same work, but on the main thread before the
    // event loop (the A/B baseline for the parallel start).
    let warm = if std::env::var_os("R3_SERIAL_INIT").is_some() {
        let w = warm();
        std::thread::spawn(move || w)
    } else {
        std::thread::spawn(warm)
    };
    let scene = Scene::new();
    let n = scene.params.len();
    let app = App {
        rec,
        scene,
        cx: None,
        warm: Some(warm),
        gfx: None,
        fcx: None,
        lcx: parley::LayoutContext::new(),
        layouts: (0..n.min(VISIBLE)).map(|_| None).collect(),
    };
    phase("main_ready");
    EventLoop::new()
        .expect("event loop")
        .run_app(app)
        .expect("run");
}
