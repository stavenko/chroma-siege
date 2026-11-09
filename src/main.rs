mod components;
mod config;
mod draw;
mod ecs;
mod game;
mod math;
mod render;
mod save;
mod ui;

use game::{Persist, Phase, Run, UiAction};
use math::Vec2;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use web_sys::{HtmlCanvasElement, KeyboardEvent, MouseEvent};

const MAX_DT: f32 = 0.05;

fn request_frame(cb: &Closure<dyn FnMut(f64)>) {
    web_sys::window()
        .expect("no window")
        .request_animation_frame(cb.as_ref().unchecked_ref())
        .expect("requestAnimationFrame failed");
}

fn main() {
    console_error_panic_hook::set_once();
    let window = web_sys::window().expect("no window");
    let document = window.document().expect("no document");
    let canvas: HtmlCanvasElement = document
        .get_element_by_id("game")
        .expect("#game not found")
        .dyn_into()
        .expect("#game is not a canvas");

    let actions: Rc<RefCell<Vec<UiAction>>> = Rc::new(RefCell::new(Vec::new()));
    let ui = ui::Ui::new(&document, actions.clone());
    let mut renderer = render::Renderer::new(&canvas);

    {
        let queue = actions.clone();
        let target = canvas.clone();
        let cb = Closure::<dyn FnMut(MouseEvent)>::new(move |ev: MouseEvent| {
            let r = target.get_bounding_client_rect();
            let x = (ev.client_x() as f64 - r.left()) / r.width() * config::W as f64;
            let y = (ev.client_y() as f64 - r.top()) / r.height() * config::H as f64;
            queue.borrow_mut().push(UiAction::Click(Vec2::new(x as f32, y as f32)));
        });
        canvas
            .add_event_listener_with_callback("click", cb.as_ref().unchecked_ref())
            .expect("addEventListener failed");
        cb.forget();
    }
    {
        let queue = actions.clone();
        let cb = Closure::<dyn FnMut(KeyboardEvent)>::new(move |ev: KeyboardEvent| {
            if matches!(ev.key().as_str(), "p" | "P" | "з" | "З" | "Escape") {
                queue.borrow_mut().push(UiAction::TogglePause);
            }
        });
        window
            .add_event_listener_with_callback("keydown", cb.as_ref().unchecked_ref())
            .expect("addEventListener failed");
        cb.forget();
    }

    let world = game::create_world(save::load(), js_sys::Date::now() as u64);

    let frame: Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>> = Rc::new(RefCell::new(None));
    let frame_handle = frame.clone();
    let mut last: Option<f64> = None;
    *frame_handle.borrow_mut() = Some(Closure::new(move |now: f64| {
        let dt = last.map_or(0.0, |l| ((now - l) / 1000.0) as f32).min(MAX_DT);
        last = Some(now);

        let pending: Vec<UiAction> = actions.borrow_mut().drain(..).collect();
        for a in pending {
            game::handle_action(&world, a);
        }
        let playing = world.res::<Run>().phase == Phase::Playing;
        if playing {
            game::update(&world, dt);
        }

        renderer.begin();
        draw::draw(&mut renderer, &world);
        renderer.flush([0.09, 0.10, 0.12, 1.0]);
        ui.sync(&world);

        {
            let mut p = world.res_mut::<Persist>();
            if p.dirty {
                save::store(&p.data);
                p.dirty = false;
            }
        }

        request_frame(frame.borrow().as_ref().expect("frame closure missing"));
    }));
    request_frame(frame_handle.borrow().as_ref().expect("frame closure missing"));
}
