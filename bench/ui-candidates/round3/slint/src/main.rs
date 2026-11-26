//! U1-B tour 3 : la scène commune avec Slint (référence de comparaison).
use r3_common::{Recorder, Scene};
use slint::{Model, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::rc::Rc;

slint::slint! {
    #[edition = "1.19"]
    import { ListView, Slider } from "std-widgets.slint";
    export struct Row { label: string, value: float }
    export component Main inherits Window {
        preferred-width: 1280px;
        preferred-height: 800px;
        in property <[Row]> rows;
        ListView {
            for row in rows: HorizontalLayout {
                height: 16px;
                Text { text: row.label; width: 200px; }
                Slider { value: row.value; minimum: 0; maximum: 1; }
            }
        }
    }
}

fn main() {
    let rec = Rc::new(RefCell::new(Recorder::new("slint")));
    let scene = Rc::new(RefCell::new(Scene::new()));
    let ui = Main::new().expect("window");
    let rows: Rc<VecModel<Row>> = Rc::new(VecModel::default());
    {
        let s = scene.borrow();
        for i in 0..s.params.len() {
            rows.push(Row {
                label: SharedString::from(s.label(i)),
                value: s.params[i],
            });
        }
    }
    ui.set_rows(ModelRc::from(rows.clone()));
    let weak = ui.as_weak();
    ui.window()
        .set_rendering_notifier(move |state, _| {
            if let slint::RenderingState::AfterRendering = state {
                if rec.borrow_mut().tick() {
                    let _ = slint::quit_event_loop();
                    return;
                }
                let mut s = scene.borrow_mut();
                s.step();
                // The parameter that moved is the one `step` just changed.
                for k in 0..20.min(s.params.len()) {
                    let r = Row {
                        label: SharedString::from(s.label(k)),
                        value: s.params[k],
                    };
                    if rows
                        .row_data(k)
                        .map(|o| o.value != r.value)
                        .unwrap_or(false)
                    {
                        rows.set_row_data(k, r);
                    }
                }
                if let Some(u) = weak.upgrade() {
                    u.window().request_redraw();
                }
            }
        })
        .expect("rendering notifier");
    ui.run().expect("event loop");
}
