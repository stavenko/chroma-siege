//! DOM HUD, upgrade buttons and overlays. Writes to the DOM only on change.

use crate::components::Stat;
use crate::config::*;
use crate::ecs::World;
use crate::game::{stats_for, Persist, Phase, Run, UiAction};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use web_sys::{Document, Element};

pub struct Ui {
    doc: Document,
    cache: RefCell<HashMap<String, String>>,
}

fn run_id(s: Stat) -> &'static str {
    match s {
        Stat::Slots => "run-slots",
        Stat::Damage => "run-damage",
        Stat::Health => "run-health",
        Stat::Regen => "run-regen",
    }
}

fn perm_id(s: Stat) -> &'static str {
    match s {
        Stat::Slots => "perm-slots",
        Stat::Damage => "perm-damage",
        Stat::Health => "perm-health",
        Stat::Regen => "perm-regen",
    }
}

fn stat_name(s: Stat) -> &'static str {
    match s {
        Stat::Slots => "Слоты",
        Stat::Damage => "Урон",
        Stat::Health => "Здоровье",
        Stat::Regen => "Реген",
    }
}

fn stat_value(s: Stat, level: u32) -> String {
    match s {
        Stat::Slots => slots_for(level).to_string(),
        Stat::Damage => format!("x{:.2}", dmg_mult_for(level)),
        Stat::Health => format!("{:.0}", max_hp_for(level)),
        Stat::Regen => format!("{:.1}/с", regen_for(level)),
    }
}

fn upgrade_label(s: Stat, level: u32, cost: u64, currency: &str) -> String {
    if is_maxed(s, level) {
        format!("{}\n{} · макс.", stat_name(s), stat_value(s, level))
    } else {
        format!("{}\n{} → {} · {}{}", stat_name(s), stat_value(s, level), stat_value(s, level + 1), cost, currency)
    }
}

fn fmt_time(t: f32) -> String {
    let s = t.max(0.0) as u32;
    format!("{:02}:{:02}", s / 60, s % 60)
}

impl Ui {
    pub fn new(doc: &Document, actions: Rc<RefCell<Vec<UiAction>>>) -> Self {
        let ui = Ui { doc: doc.clone(), cache: RefCell::new(HashMap::new()) };
        let mut bindings: Vec<(&'static str, UiAction)> = vec![
            ("btn-start", UiAction::Start),
            ("btn-menu", UiAction::ToMenu),
            ("btn-pause", UiAction::TogglePause),
            ("btn-resume", UiAction::TogglePause),
        ];
        for s in Stat::ALL {
            bindings.push((run_id(s), UiAction::RunUpgrade(s)));
            bindings.push((perm_id(s), UiAction::PermUpgrade(s)));
        }
        for (id, action) in bindings {
            let queue = actions.clone();
            let cb = Closure::<dyn FnMut()>::new(move || queue.borrow_mut().push(action));
            ui.el(id)
                .add_event_listener_with_callback("click", cb.as_ref().unchecked_ref())
                .expect("addEventListener failed");
            cb.forget();
        }
        ui
    }

    fn el(&self, id: &str) -> Element {
        self.doc.get_element_by_id(id).unwrap_or_else(|| panic!("element #{id} not found"))
    }

    fn changed(&self, key: String, value: &str) -> bool {
        let mut cache = self.cache.borrow_mut();
        if cache.get(&key).map(String::as_str) == Some(value) {
            return false;
        }
        cache.insert(key, value.to_string());
        true
    }

    fn text(&self, id: &str, value: String) {
        if self.changed(format!("{id}#text"), &value) {
            self.el(id).set_text_content(Some(&value));
        }
    }

    fn flag(&self, id: &str, attr: &str, on: bool) {
        if self.changed(format!("{id}#{attr}"), if on { "1" } else { "0" }) {
            self.el(id).toggle_attribute_with_force(attr, on).expect("toggleAttribute failed");
        }
    }

    pub fn sync(&self, world: &World) {
        let run = world.res::<Run>();
        let persist = world.res::<Persist>();
        let perm = persist.data.perm;
        let total = perm.plus(run.levels);
        let stats = stats_for(total);
        let playing = run.phase == Phase::Playing;
        let in_battle = matches!(run.phase, Phase::Playing | Phase::Paused);

        self.text("hud-hp", format!("{:.0}/{:.0}", run.hp.max(0.0), stats.max_hp));
        self.text("hud-cash", run.cash.to_string());
        self.text("hud-coins", persist.data.coins.to_string());
        self.text("hud-level", format!("x{:.2}", enemy_level(run.time)));
        self.text("hud-time", fmt_time(run.time));
        self.text("btn-pause", if run.phase == Phase::Paused { "Продолжить" } else { "Пауза" }.to_string());
        self.flag("btn-pause", "disabled", !in_battle);

        for s in Stat::ALL {
            let run_lvl = run.levels.get(s);
            let cost = run_cost(s, run_lvl);
            let tot = total.get(s);
            self.text(run_id(s), upgrade_label(s, tot, cost, " кеш"));
            self.flag(run_id(s), "disabled", !playing || is_maxed(s, tot) || run.cash < cost);

            let p_lvl = perm.get(s);
            let p_cost = perm_cost(s, p_lvl);
            self.text(perm_id(s), upgrade_label(s, p_lvl, p_cost, " мон."));
            self.flag(perm_id(s), "disabled", is_maxed(s, p_lvl) || persist.data.coins < p_cost);
        }

        self.flag("overlay", "hidden", playing);
        self.flag("panel-menu", "hidden", run.phase != Phase::Menu);
        self.flag("panel-paused", "hidden", run.phase != Phase::Paused);
        self.flag("panel-gameover", "hidden", run.phase != Phase::GameOver);

        self.text("menu-coins", persist.data.coins.to_string());
        self.text("menu-best", fmt_time(persist.data.best_time));
        self.text("menu-runs", persist.data.runs.to_string());
        if let Some(r) = &run.result {
            self.text("go-time", fmt_time(r.time));
            self.text("go-kills", r.kills.to_string());
            self.text("go-coins", r.coins.to_string());
        }
    }
}
