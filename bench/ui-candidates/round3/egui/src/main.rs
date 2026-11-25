//! U1-B tour 3 : la scène commune avec egui (eframe, wgpu).
use r3_common::{Recorder, Scene};

struct App {
    scene: Scene,
    rec: Recorder,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        use eframe::egui;
        self.scene.step();
        let n = self.scene.params.len();
        // Same row geometry as the other stacks: 16 px, no extra spacing.
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.spacing_mut().interact_size.y = 16.0;
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show_rows(ui, 16.0, n, |ui, range| {
                for i in range {
                    ui.allocate_ui(egui::vec2(ui.available_width(), 16.0), |ui| {
                        ui.horizontal(|ui| {
                            ui.label(self.scene.label(i));
                            ui.add(
                                egui::Slider::new(&mut self.scene.params[i], 0.0..=1.0)
                                    .show_value(false),
                            );
                        })
                    });
                }
            });
        if self.rec.tick() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ui.ctx().request_repaint();
    }
}

fn main() -> eframe::Result {
    let rec = Recorder::new("egui");
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1280.0, 800.0]),
        ..Default::default()
    };
    eframe::run_native(
        "r3-egui",
        options,
        Box::new(|_cc| {
            Ok(Box::new(App {
                scene: Scene::new(),
                rec,
            }))
        }),
    )
}
