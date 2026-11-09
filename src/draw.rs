//! Scene rendering: reads the world, emits primitives.

use crate::components::*;
use crate::config::*;
use crate::ecs::World;
use crate::game::{arsenal_stats, first_free_slot, Deck, Phase, Run};
use crate::math::Vec2;
use crate::render::{Color, Renderer};

const BG: Color = [0.09, 0.10, 0.12, 1.0];
const GRID: Color = [1.0, 1.0, 1.0, 0.04];
const STRIP: Color = [0.85, 0.2, 0.2, 0.16];
const STRIP_EDGE: Color = [0.95, 0.3, 0.3, 0.6];
const ARSENAL_BG: Color = [0.13, 0.14, 0.17, 1.0];
const SLOT: Color = [1.0, 1.0, 1.0, 0.10];
const BAR_BG: Color = [0.0, 0.0, 0.0, 0.55];
pub const ARMORED: Color = [0.45, 0.47, 0.52, 1.0];

pub fn hue_color(h: Hue) -> Color {
    match h {
        Hue::Red => [0.92, 0.33, 0.33, 1.0],
        Hue::Green => [0.33, 0.80, 0.45, 1.0],
        Hue::Blue => [0.33, 0.55, 0.95, 1.0],
    }
}

fn shade(c: Color, k: f32) -> Color {
    [c[0] * k, c[1] * k, c[2] * k, c[3]]
}

fn with_alpha(c: Color, a: f32) -> Color {
    [c[0], c[1], c[2], c[3] * a]
}

pub fn draw(r: &mut Renderer, world: &World) {
    let stats = arsenal_stats(world);
    let run = world.res::<Run>();

    r.rect(0.0, 0.0, W, H, BG);
    for c in 1..COLS {
        r.rect(c as f32 * CELL - 0.5, 0.0, 1.0, STRIP_BOTTOM, GRID);
    }
    r.rect(0.0, STRIP_TOP, W, STRIP_BOTTOM - STRIP_TOP, STRIP);
    r.rect(0.0, STRIP_TOP, W, 2.0, STRIP_EDGE);

    // blocks
    {
        let blocks = world.read::<Block>();
        let pos = world.read::<Pos>();
        let health = world.read::<Health>();
        for (e, b) in blocks.iter() {
            let p = pos.get(e).expect("block without Pos").0;
            let h = health.get(e).expect("block without Health");
            draw_block(r, p, b.kind, h.hp / h.max);
        }
    }

    // arsenal
    r.rect(0.0, ARSENAL_TOP, W, ARSENAL_BOTTOM - ARSENAL_TOP, ARSENAL_BG);
    let slot_w = W / stats.slots as f32;
    for i in 0..stats.slots {
        r.frame_c(Vec2::new(slot_x(i, stats.slots), ARSENAL_Y), slot_w - 8.0, 70.0, 1.5, SLOT);
    }
    {
        let guns = world.read::<Gun>();
        for (_, g) in guns.iter() {
            let c = Vec2::new(slot_x(g.slot, stats.slots), ARSENAL_Y);
            draw_gun(r, c, g.spec, g.aim, Some(g.shots_left as f32 / g.shots_max as f32));
        }
    }
    let hp_frac = (run.hp / stats.max_hp).clamp(0.0, 1.0);
    r.rect(8.0, HP_BAR_Y, W - 16.0, 6.0, BAR_BG);
    r.rect(8.0, HP_BAR_Y, (W - 16.0) * hp_frac, 6.0, [1.0 - hp_frac * 0.7, 0.3 + hp_frac * 0.5, 0.3, 1.0]);

    // projectiles
    {
        let bullets = world.read::<Bullet>();
        let pos = world.read::<Pos>();
        for (e, b) in bullets.iter() {
            let p = pos.get(e).expect("bullet without Pos").0;
            r.circle(p, BULLET_R, [1.0, 1.0, 1.0, 1.0]);
            r.circle(p, BULLET_R - 1.5, hue_color(b.hue));
        }
    }
    {
        let shells = world.read::<Shell>();
        for (_, s) in shells.iter() {
            let col = hue_color(s.hue);
            r.ring(s.to, MORTAR_RADIUS, 1.5, with_alpha(col, 0.25));
            let p = s.from.lerp(s.to, s.t) - Vec2::new(0.0, SHELL_ARC * 4.0 * s.t * (1.0 - s.t));
            r.circle(p, 8.0, shade(col, 0.4));
            r.circle(p, 6.0, col);
        }
    }

    // effects
    {
        let effects = world.read::<Effect>();
        let pos = world.read::<Pos>();
        for (e, fx) in effects.iter() {
            let p = pos.get(e).expect("effect without Pos").0;
            let k = (fx.ttl / fx.max_ttl).clamp(0.0, 1.0);
            let radius = fx.r0 + (fx.r1 - fx.r0) * (1.0 - k);
            r.circle(p, radius, with_alpha(fx.color, k));
        }
    }

    // stacks
    let can_take = run.phase == Phase::Playing && first_free_slot(world, stats.slots).is_some();
    let deck = world.res::<Deck>();
    for (i, &x) in STACK_XS.iter().enumerate() {
        let c = Vec2::new(x, STACK_Y);
        let stack = &deck.stacks[i];
        if stack.is_empty() {
            r.frame_c(c, STACK_W, STACK_H, 2.0, SLOT);
            continue;
        }
        if let Some(next) = stack.get(1) {
            let nc = c + Vec2::new(6.0, -6.0);
            r.rect_c(nc, STACK_W, STACK_H, shade(hue_color(next.hue), 0.35));
        }
        let top = stack[0];
        r.rect_c(c, STACK_W, STACK_H, [0.17, 0.18, 0.22, 1.0]);
        r.frame_c(c, STACK_W, STACK_H, 2.0, shade(hue_color(top.hue), 0.8));
        draw_gun(r, c, top, Vec2::new(0.0, -1.0), None);
        if !can_take {
            r.rect_c(c + Vec2::new(3.0, -3.0), STACK_W + 8.0, STACK_H + 8.0, [0.09, 0.10, 0.12, 0.6]);
        }
    }
}

fn draw_block(r: &mut Renderer, p: Vec2, kind: BlockKind, hp_frac: f32) {
    match kind {
        BlockKind::Attacker(h) => {
            let c = hue_color(h);
            r.rect_c(p, BLOCK, BLOCK, c);
            r.frame_c(p, BLOCK, BLOCK, 2.0, shade(c, 0.6));
            // a fang at the bottom marks attackers
            let y = p.y + BLOCK / 2.0 - 6.0;
            r.tri(Vec2::new(p.x - 8.0, y - 10.0), Vec2::new(p.x + 8.0, y - 10.0), Vec2::new(p.x, y), shade(c, 0.5));
        }
        BlockKind::Armored => {
            r.rect_c(p, BLOCK, BLOCK, ARMORED);
            r.rect_c(p, BLOCK - 14.0, BLOCK - 14.0, shade(ARMORED, 0.75));
            for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                r.rect_c(p + Vec2::new(dx * 19.0, dy * 19.0), 4.0, 4.0, shade(ARMORED, 1.4));
            }
        }
    }
    if hp_frac < 1.0 {
        let (x, y, w) = (p.x - BLOCK / 2.0 + 4.0, p.y - BLOCK / 2.0 + 4.0, BLOCK - 8.0);
        r.rect(x, y, w, 4.0, BAR_BG);
        r.rect(x, y, w * hp_frac.max(0.0), 4.0, [1.0, 1.0, 1.0, 0.85]);
    }
}

fn draw_gun(r: &mut Renderer, c: Vec2, spec: GunSpec, aim: Vec2, shots: Option<f32>) {
    let col = hue_color(spec.hue);
    let dark = shade(col, 0.5);
    match spec.kind {
        GunKind::Direct => {
            r.line(c, c + aim * 28.0, 8.0, dark);
            r.rect_c(c, 34.0, 34.0, col);
            r.frame_c(c, 34.0, 34.0, 2.0, dark);
        }
        GunKind::Mortar => {
            r.line(c, c + aim * 18.0, 16.0, dark);
            r.circle(c, 19.0, col);
            r.ring(c, 19.0, 2.0, dark);
        }
    }
    if let Some(f) = shots {
        let (x, y, w) = (c.x - 26.0, c.y + 24.0, 52.0);
        r.rect(x, y, w, 4.0, BAR_BG);
        r.rect(x, y, w * f, 4.0, [0.95, 0.85, 0.4, 1.0]);
    }
}
