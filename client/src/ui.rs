//! DOM HUD, upgrade buttons, quests and overlays. Writes to the DOM only on change.

use crate::components::{Hue, Stat};
use crate::config::*;
use crate::ecs::World;
use crate::game::{stats_for, Link, Persist, Phase, Quests, Run, UiAction};
use chroma_protocol::{QuestView, Stats};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use web_sys::{Document, Element, MouseEvent};

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

fn reward_text(q: &QuestView) -> String {
    if q.reward.coins > 0 {
        format!("+{} кеш, +{} мон.", q.reward.cash, q.reward.coins)
    } else {
        format!("+{} кеш", q.reward.cash)
    }
}

/// Built only from enums and numbers, so it needs no escaping.
fn quests_html(list: &[QuestView], pending: &[u32], can_claim: bool) -> String {
    let mut html = String::new();
    for q in list {
        let done = q.completed_at.is_some();
        let pct = q.progress as f32 / q.target as f32 * 100.0;
        html.push_str(&format!(
            "<div class=\"quest{}\"><div>{}</div><div class=\"bar\"><i style=\"width:{:.0}%\"></i></div><div class=\"meta\">{}/{} · {}</div>",
            if done { " done" } else { "" },
            q.kind.describe(q.target),
            pct,
            q.progress,
            q.target,
            reward_text(q)
        ));
        if done {
            let disabled = if !can_claim || pending.contains(&q.id) { " disabled" } else { "" };
            html.push_str(&format!("<button data-quest=\"{}\"{}>Забрать</button>", q.id, disabled));
        }
        html.push_str("</div>");
    }
    html
}

fn stats_text(s: &Stats) -> String {
    let shots = s.shots_direct + s.shots_mortar;
    let accuracy = if shots == 0 { 0.0 } else { s.hits as f32 / shots as f32 * 100.0 };
    let k = |h: Hue| s.kills_by_hue[h.index()];
    format!(
        "Волн: {}\nВыстрелов: {} (прямая {}, мортира {})\nПопаданий: {} ({:.0}% от выстрелов)\nУбито: красных {}, зелёных {}, синих {}, серых {}\nДобито: прямой {}, мортирой {}, полосой {}\nПушек взято: {} · улучшений: {}\nУрон по арсеналу: {:.0}\nЗаданий выполнено: {}",
        s.waves,
        shots,
        s.shots_direct,
        s.shots_mortar,
        s.hits,
        accuracy,
        k(Hue::Red),
        k(Hue::Green),
        k(Hue::Blue),
        s.kills_armored,
        s.kills_direct,
        s.kills_mortar,
        s.kills_strip,
        s.picks,
        s.upgrades,
        s.damage_taken,
        s.quests_completed
    )
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
        // Quest buttons are re-rendered often, so one delegated listener handles them all.
        let queue = actions.clone();
        let cb = Closure::<dyn FnMut(MouseEvent)>::new(move |ev: MouseEvent| {
            let Some(target) = ev.target().and_then(|t| t.dyn_into::<Element>().ok()) else {
                return;
            };
            let Some(button) = target.closest("button[data-quest]").expect("closest() failed") else {
                return;
            };
            let raw = button.get_attribute("data-quest").expect("data-quest vanished");
            let id: u32 = raw.parse().unwrap_or_else(|e| panic!("bad data-quest {raw}: {e}"));
            queue.borrow_mut().push(UiAction::ClaimQuest(id));
        });
        ui.el("quests")
            .add_event_listener_with_callback("click", cb.as_ref().unchecked_ref())
            .expect("addEventListener failed");
        cb.forget();
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

    fn html(&self, id: &str, value: String) {
        if self.changed(format!("{id}#html"), &value) {
            self.el(id).set_inner_html(&value);
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
        let link = world.res::<Link>();
        let quests = world.res::<Quests>();
        let perm = persist.data.perm;
        let total = perm.plus(run.levels);
        let stats = stats_for(total);
        let playing = run.phase == Phase::Playing;

        self.text("hud-hp", format!("{:.0}/{:.0}", run.hp.max(0.0), stats.max_hp));
        self.text("hud-cash", run.cash.to_string());
        self.text("hud-coins", persist.data.coins.to_string());
        self.text("hud-wave", run.wave.to_string());
        self.text("hud-level", format!("x{:.2}", enemy_level(run.time)));
        self.text("hud-time", fmt_time(run.time));
        self.text("hud-link", if link.connected { "онлайн" } else { "нет связи" }.to_string());
        self.flag("hud-link", "data-offline", !link.connected);
        self.text("btn-pause", if run.phase == Phase::Paused { "Продолжить" } else { "Пауза" }.to_string());
        self.flag("btn-pause", "disabled", !run.in_battle() || (run.phase == Phase::Paused && !link.connected));

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

        self.html("quests", quests_html(&quests.list, &quests.pending_claims, playing && link.connected));
        let stats_str = quests.stats.as_ref().map(stats_text).unwrap_or_else(|| "Статистики с сервера пока нет".to_string());
        self.text("pause-stats", stats_str.clone());
        self.text("go-stats", stats_str);

        self.flag("overlay", "hidden", playing);
        self.flag("panel-menu", "hidden", run.phase != Phase::Menu);
        self.flag("panel-paused", "hidden", run.phase != Phase::Paused);
        self.flag("panel-gameover", "hidden", run.phase != Phase::GameOver);

        let link_error = link.last_error.clone().unwrap_or_default();
        self.text(
            "menu-link",
            if link.connected { String::new() } else { format!("Нет связи с игровым сервером, переподключаюсь… {link_error}") },
        );
        self.flag("btn-start", "disabled", !link.connected);
        self.text(
            "pause-link",
            if link.connected {
                String::new()
            } else {
                format!("Связь с сервером потеряна, бой продолжится после переподключения. Задания и статистика этого боя на сервере сброшены. {link_error}")
            },
        );
        self.flag("btn-resume", "disabled", !link.connected);

        self.text("menu-coins", persist.data.coins.to_string());
        self.text("menu-best", fmt_time(persist.data.best_time));
        self.text("menu-runs", persist.data.runs.to_string());
        if let Some(r) = &run.result {
            self.text("go-time", fmt_time(r.time));
            self.text("go-wave", r.wave.to_string());
            self.text("go-kills", r.kills.to_string());
            self.text("go-coins", r.coins.to_string());
        }
    }
}
