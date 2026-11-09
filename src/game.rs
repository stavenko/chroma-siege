//! Resources, actions and systems.

use crate::components::*;
use crate::config::*;
use crate::ecs::{Entity, World};
use crate::math::Vec2;
use crate::save::SaveData;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Menu,
    Playing,
    Paused,
    GameOver,
}

#[derive(Clone, Copy, Debug)]
pub enum UiAction {
    Click(Vec2),
    Start,
    ToMenu,
    TogglePause,
    RunUpgrade(Stat),
    PermUpgrade(Stat),
}

// ---------- resources ----------

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    pub fn f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
    pub fn chance(&mut self, p: f32) -> bool {
        self.f32() < p
    }
    pub fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[(self.next_u64() % items.len() as u64) as usize]
    }
}

pub struct Persist {
    pub data: SaveData,
    pub dirty: bool,
}

pub struct Deck {
    /// Index 0 is the top (visible) gun, index 1 is the next one.
    pub stacks: [Vec<GunSpec>; 3],
}

fn random_gun(rng: &mut Rng) -> GunSpec {
    let kind = if rng.chance(MORTAR_CHANCE) { GunKind::Mortar } else { GunKind::Direct };
    GunSpec { kind, hue: rng.pick(&Hue::ALL) }
}

impl Deck {
    pub fn empty() -> Self {
        Deck { stacks: [Vec::new(), Vec::new(), Vec::new()] }
    }
    fn new(rng: &mut Rng) -> Self {
        Deck { stacks: std::array::from_fn(|_| vec![random_gun(rng), random_gun(rng)]) }
    }
    fn take(&mut self, i: usize, rng: &mut Rng) -> GunSpec {
        let s = &mut self.stacks[i];
        let top = s.remove(0);
        s.push(random_gun(rng));
        top
    }
}

pub struct RunResult {
    pub time: f32,
    pub kills: u32,
    pub coins: u64,
}

pub struct Run {
    pub phase: Phase,
    pub time: f32,
    pub cash: u64,
    pub coins: u64,
    pub kills: u32,
    pub levels: Levels,
    pub hp: f32,
    pub spawn_timer: f32,
    pub result: Option<RunResult>,
}

impl Run {
    fn new(phase: Phase) -> Self {
        Run {
            phase,
            time: 0.0,
            cash: 0,
            coins: 0,
            kills: 0,
            levels: Levels::default(),
            hp: 0.0,
            spawn_timer: 0.0,
            result: None,
        }
    }
}

pub struct ArsenalStats {
    pub slots: usize,
    pub dmg_mult: f32,
    pub max_hp: f32,
    pub regen: f32,
}

pub fn stats_for(l: Levels) -> ArsenalStats {
    ArsenalStats {
        slots: slots_for(l.slots),
        dmg_mult: dmg_mult_for(l.damage),
        max_hp: max_hp_for(l.health),
        regen: regen_for(l.regen),
    }
}

/// Permanent levels plus levels bought during the current battle.
pub fn total_levels(world: &World) -> Levels {
    let perm = world.res::<Persist>().data.perm;
    perm.plus(world.res::<Run>().levels)
}

pub fn arsenal_stats(world: &World) -> ArsenalStats {
    stats_for(total_levels(world))
}

pub fn first_free_slot(world: &World, slots: usize) -> Option<usize> {
    let guns = world.read::<Gun>();
    let mut used = vec![false; slots];
    for (_, g) in guns.iter() {
        used[g.slot] = true;
    }
    used.iter().position(|u| !u)
}

// ---------- world setup ----------

pub fn create_world(save: SaveData, seed: u64) -> World {
    let mut w = World::new();
    w.register::<Pos>();
    w.register::<Block>();
    w.register::<Health>();
    w.register::<AttackTimer>();
    w.register::<Gun>();
    w.register::<Bullet>();
    w.register::<Shell>();
    w.register::<Effect>();
    w.insert_resource(Rng::new(seed));
    w.insert_resource(Persist { data: save, dirty: false });
    w.insert_resource(Run::new(Phase::Menu));
    w.insert_resource(Deck::empty());
    w
}

fn start_run(world: &World) {
    world.clear_entities();
    *world.res_mut::<Run>() = Run::new(Phase::Playing);
    let max_hp = arsenal_stats(world).max_hp;
    world.res_mut::<Run>().hp = max_hp;
    {
        let mut rng = world.res_mut::<Rng>();
        *world.res_mut::<Deck>() = Deck::new(&mut rng);
    }
    for i in 0..PRESPAWN_ROWS {
        spawn_row(world, CELL / 2.0 + i as f32 * CELL, 1.0);
    }
    let mut p = world.res_mut::<Persist>();
    p.data.runs += 1;
    p.dirty = true;
}

fn spawn_row(world: &World, y: f32, power: f32) {
    let mut rng = world.res_mut::<Rng>();
    for col in 0..COLS {
        if !rng.chance(FILL_PROB) {
            continue;
        }
        let kind = if rng.chance(ARMORED_PROB) {
            BlockKind::Armored
        } else {
            BlockKind::Attacker(rng.pick(&Hue::ALL))
        };
        let base_hp = match kind {
            BlockKind::Armored => ARMORED_HP,
            BlockKind::Attacker(_) => ATTACKER_HP,
        };
        let e = world.spawn();
        world.insert(e, Pos(Vec2::new((col as f32 + 0.5) * CELL, y)));
        world.insert(e, Block { kind, col, power });
        world.insert(e, Health { hp: base_hp * power, max: base_hp * power });
        if let BlockKind::Attacker(_) = kind {
            world.insert(e, AttackTimer(ATTACK_INTERVAL * 0.5));
        }
    }
}

fn spawn_effect(world: &World, p: Vec2, r0: f32, r1: f32, ttl: f32, color: [f32; 4]) {
    let e = world.spawn();
    world.insert(e, Pos(p));
    world.insert(e, Effect { ttl, max_ttl: ttl, r0, r1, color });
}

// ---------- actions ----------

pub fn handle_action(world: &World, action: UiAction) {
    let phase = world.res::<Run>().phase;
    match action {
        UiAction::Click(p) if phase == Phase::Playing => click(world, p),
        UiAction::Click(_) => {}
        UiAction::Start if matches!(phase, Phase::Menu | Phase::GameOver) => start_run(world),
        UiAction::Start => {}
        UiAction::ToMenu if phase == Phase::GameOver => {
            world.clear_entities();
            *world.res_mut::<Deck>() = Deck::empty();
            world.res_mut::<Run>().phase = Phase::Menu;
        }
        UiAction::ToMenu => {}
        UiAction::TogglePause => {
            let mut run = world.res_mut::<Run>();
            run.phase = match run.phase {
                Phase::Playing => Phase::Paused,
                Phase::Paused => Phase::Playing,
                other => other,
            };
        }
        UiAction::RunUpgrade(stat) if phase == Phase::Playing => run_upgrade(world, stat),
        UiAction::RunUpgrade(_) => {}
        UiAction::PermUpgrade(stat) if phase == Phase::Menu => perm_upgrade(world, stat),
        UiAction::PermUpgrade(_) => {}
    }
}

fn click(world: &World, p: Vec2) {
    let Some(stack) = STACK_XS.iter().position(|&x| {
        (p.x - x).abs() <= STACK_W / 2.0 && (p.y - STACK_Y).abs() <= STACK_H / 2.0
    }) else {
        return;
    };
    let slots = arsenal_stats(world).slots;
    let Some(slot) = first_free_slot(world, slots) else {
        return;
    };
    let spec = {
        let mut rng = world.res_mut::<Rng>();
        world.res_mut::<Deck>().take(stack, &mut rng)
    };
    let params = match spec.kind {
        GunKind::Direct => DIRECT,
        GunKind::Mortar => MORTAR,
    };
    let e = world.spawn();
    world.insert(
        e,
        Gun {
            spec,
            shots_left: params.shots,
            shots_max: params.shots,
            cooldown: 0.0,
            slot,
            aim: Vec2::new(0.0, -1.0),
        },
    );
}

fn run_upgrade(world: &World, stat: Stat) {
    let before = arsenal_stats(world);
    let total = total_levels(world).get(stat);
    if is_maxed(stat, total) {
        return;
    }
    {
        let mut run = world.res_mut::<Run>();
        let cost = run_cost(stat, run.levels.get(stat));
        if run.cash < cost {
            return;
        }
        run.cash -= cost;
        run.levels.inc(stat);
    }
    if stat == Stat::Health {
        let after = arsenal_stats(world);
        world.res_mut::<Run>().hp += after.max_hp - before.max_hp;
    }
}

fn perm_upgrade(world: &World, stat: Stat) {
    let mut p = world.res_mut::<Persist>();
    let lvl = p.data.perm.get(stat);
    if is_maxed(stat, lvl) {
        return;
    }
    let cost = perm_cost(stat, lvl);
    if p.data.coins < cost {
        return;
    }
    p.data.coins -= cost;
    p.data.perm.inc(stat);
    p.dirty = true;
}

// ---------- systems ----------

pub fn update(world: &World, dt: f32) {
    world.res_mut::<Run>().time += dt;
    let stats = arsenal_stats(world);

    spawn_system(world, dt);
    movement_system(world, dt);
    gun_system(world, dt, &stats);
    world.maintain();
    bullet_system(world, dt);
    world.maintain();
    shell_system(world, dt);
    world.maintain();
    strip_system(world, dt, &stats);
    regen_system(world, dt, &stats);
    death_system(world);
    world.maintain();
    effect_system(world, dt);
    world.maintain();
    game_over_system(world);
}

fn spawn_system(world: &World, dt: f32) {
    let power = {
        let mut run = world.res_mut::<Run>();
        run.spawn_timer -= dt;
        if run.spawn_timer > 0.0 {
            return;
        }
        run.spawn_timer += ROW_INTERVAL;
        enemy_level(run.time)
    };
    spawn_row(world, SPAWN_Y, power);
}

/// Blocks slide down until they rest on the strip bottom or on a block below.
fn movement_system(world: &World, dt: f32) {
    let blocks = world.read::<Block>();
    let mut pos = world.write::<Pos>();
    let mut columns: Vec<Vec<(Entity, f32)>> = vec![Vec::new(); COLS];
    for (e, b) in blocks.iter() {
        let y = pos.get(e).expect("block without Pos").0.y;
        columns[b.col].push((e, y));
    }
    for col in columns.iter_mut() {
        col.sort_by(|a, b| b.1.partial_cmp(&a.1).expect("NaN block y"));
        let mut limit = STRIP_BOTTOM - CELL / 2.0;
        for &(e, y) in col.iter() {
            let ny = (y + BLOCK_SPEED * dt).min(limit);
            pos.get_mut(e).expect("block without Pos").0.y = ny;
            limit = ny - CELL;
        }
    }
}

fn gun_system(world: &World, dt: f32, stats: &ArsenalStats) {
    let targets: Vec<(Vec2, Hue)> = {
        let blocks = world.read::<Block>();
        let pos = world.read::<Pos>();
        blocks
            .iter()
            .filter_map(|(e, b)| match b.kind {
                BlockKind::Attacker(h) => Some((pos.get(e).expect("block without Pos").0, h)),
                BlockKind::Armored => None,
            })
            .collect()
    };

    let mut bullets = Vec::new();
    let mut shells = Vec::new();
    {
        let mut guns = world.write::<Gun>();
        for (e, g) in guns.iter_mut() {
            g.cooldown = (g.cooldown - dt).max(0.0);
            let gp = Vec2::new(slot_x(g.slot, stats.slots), ARSENAL_Y);
            let target = targets
                .iter()
                .filter(|(_, h)| *h == g.spec.hue)
                .min_by(|a, b| gp.dist2(a.0).partial_cmp(&gp.dist2(b.0)).expect("NaN distance"));
            let Some(&(tp, _)) = target else {
                continue;
            };
            g.aim = (tp - gp).norm();
            if g.cooldown > 0.0 {
                continue;
            }
            match g.spec.kind {
                GunKind::Direct => {
                    g.cooldown = DIRECT.cooldown;
                    bullets.push((
                        gp + g.aim * MUZZLE,
                        Bullet { vel: g.aim * BULLET_SPEED, hue: g.spec.hue, dmg: DIRECT.dmg * stats.dmg_mult },
                    ));
                }
                GunKind::Mortar => {
                    g.cooldown = MORTAR.cooldown;
                    shells.push(Shell { from: gp, to: tp, t: 0.0, hue: g.spec.hue, dmg: MORTAR.dmg * stats.dmg_mult });
                }
            }
            g.shots_left -= 1;
            if g.shots_left == 0 {
                world.despawn(e);
            }
        }
    }
    for (p, b) in bullets {
        let e = world.spawn();
        world.insert(e, Pos(p));
        world.insert(e, b);
    }
    for s in shells {
        let e = world.spawn();
        world.insert(e, s);
    }
}

fn block_snapshot(world: &World) -> Vec<(Entity, Vec2, BlockKind)> {
    let blocks = world.read::<Block>();
    let pos = world.read::<Pos>();
    blocks
        .iter()
        .map(|(e, b)| (e, pos.get(e).expect("block without Pos").0, b.kind))
        .collect()
}

fn apply_damage(world: &World, hits: &[(Entity, f32)]) {
    let mut health = world.write::<Health>();
    for &(e, d) in hits {
        health.get_mut(e).expect("damaged block without Health").hp -= d;
    }
}

/// Direct fire: flies straight, passes through blocks of other hues,
/// stops at the first armored block or block of its own hue.
fn bullet_system(world: &World, dt: f32) {
    let blocks = block_snapshot(world);
    let mut hits = Vec::new();
    let mut sparks = Vec::new();
    {
        let mut pos = world.write::<Pos>();
        let bullets = world.read::<Bullet>();
        for (e, b) in bullets.iter() {
            let p = pos.get_mut(e).expect("bullet without Pos");
            p.0 = p.0 + b.vel * dt;
            let pp = p.0;
            if pp.x < -20.0 || pp.x > W + 20.0 || pp.y < -40.0 || pp.y > H {
                world.despawn(e);
                continue;
            }
            let reach = BLOCK / 2.0 + BULLET_R;
            for &(be, bp, kind) in &blocks {
                let hittable = match kind {
                    BlockKind::Armored => true,
                    BlockKind::Attacker(h) => h == b.hue,
                };
                if hittable && (pp.x - bp.x).abs() <= reach && (pp.y - bp.y).abs() <= reach {
                    hits.push((be, b.dmg));
                    sparks.push(pp);
                    world.despawn(e);
                    break;
                }
            }
        }
    }
    apply_damage(world, &hits);
    for p in sparks {
        spawn_effect(world, p, 3.0, 12.0, 0.15, [1.0, 1.0, 0.85, 0.9]);
    }
}

/// Mortar: lobs over everything; the explosion hurts blocks of its hue and armored blocks.
fn shell_system(world: &World, dt: f32) {
    let mut explosions = Vec::new();
    {
        let mut shells = world.write::<Shell>();
        for (e, s) in shells.iter_mut() {
            s.t += dt / SHELL_FLIGHT;
            if s.t >= 1.0 {
                explosions.push((s.to, s.hue, s.dmg));
                world.despawn(e);
            }
        }
    }
    if explosions.is_empty() {
        return;
    }
    let blocks = block_snapshot(world);
    let mut hits = Vec::new();
    for &(at, hue, dmg) in &explosions {
        for &(be, bp, kind) in &blocks {
            let affected = match kind {
                BlockKind::Armored => true,
                BlockKind::Attacker(h) => h == hue,
            };
            // distance from explosion center to the block rectangle
            let dx = ((at.x - bp.x).abs() - BLOCK / 2.0).max(0.0);
            let dy = ((at.y - bp.y).abs() - BLOCK / 2.0).max(0.0);
            if affected && dx * dx + dy * dy <= MORTAR_RADIUS * MORTAR_RADIUS {
                hits.push((be, dmg));
            }
        }
    }
    apply_damage(world, &hits);
    for (at, hue, _) in explosions {
        let c = crate::draw::hue_color(hue);
        spawn_effect(world, at, 10.0, MORTAR_RADIUS, 0.35, [c[0], c[1], c[2], 0.55]);
    }
}

/// The strip hurts every block on it; attackers on it hit the arsenal.
fn strip_system(world: &World, dt: f32, stats: &ArsenalStats) {
    let mut arsenal_dmg = 0.0;
    let mut strikes = Vec::new();
    {
        let blocks = world.read::<Block>();
        let pos = world.read::<Pos>();
        let mut health = world.write::<Health>();
        let mut timers = world.write::<AttackTimer>();
        for (e, b) in blocks.iter() {
            let p = pos.get(e).expect("block without Pos").0;
            if p.y < STRIP_TOP || p.y > STRIP_BOTTOM {
                continue;
            }
            health.get_mut(e).expect("block without Health").hp -= STRIP_DPS * stats.dmg_mult * dt;
            if let BlockKind::Attacker(_) = b.kind {
                let t = timers.get_mut(e).expect("attacker without AttackTimer");
                t.0 -= dt;
                if t.0 <= 0.0 {
                    t.0 += ATTACK_INTERVAL;
                    arsenal_dmg += ATTACKER_DMG * b.power;
                    strikes.push(Vec2::new(p.x, STRIP_BOTTOM + 4.0));
                }
            }
        }
    }
    world.res_mut::<Run>().hp -= arsenal_dmg;
    for p in strikes {
        spawn_effect(world, p, 6.0, 26.0, 0.3, [1.0, 0.25, 0.2, 0.8]);
    }
}

fn regen_system(world: &World, dt: f32, stats: &ArsenalStats) {
    let mut run = world.res_mut::<Run>();
    if run.hp > 0.0 {
        run.hp = (run.hp + stats.regen * dt).min(stats.max_hp);
    }
}

fn death_system(world: &World) {
    let dead: Vec<(Entity, Vec2, BlockKind, f32)> = {
        let blocks = world.read::<Block>();
        let pos = world.read::<Pos>();
        let health = world.read::<Health>();
        blocks
            .iter()
            .filter(|(e, _)| health.get(*e).expect("block without Health").hp <= 0.0)
            .map(|(e, b)| (e, pos.get(e).expect("block without Pos").0, b.kind, b.power))
            .collect()
    };
    if dead.is_empty() {
        return;
    }
    let mut pops = Vec::new();
    {
        let mut run = world.res_mut::<Run>();
        let mut persist = world.res_mut::<Persist>();
        let mut rng = world.res_mut::<Rng>();
        for (e, p, kind, power) in dead {
            world.despawn(e);
            let (cash, coin_chance, color) = match kind {
                BlockKind::Armored => (ARMORED_CASH, ARMORED_COIN_CHANCE, crate::draw::ARMORED),
                BlockKind::Attacker(h) => (ATTACKER_CASH, ATTACKER_COIN_CHANCE, crate::draw::hue_color(h)),
            };
            run.cash += (cash * power.powf(CASH_LEVEL_EXP)).ceil() as u64;
            run.kills += 1;
            if rng.chance(coin_chance) {
                run.coins += 1;
                persist.data.coins += 1;
                persist.dirty = true;
            }
            pops.push((p, color));
        }
    }
    for (p, c) in pops {
        spawn_effect(world, p, BLOCK * 0.4, BLOCK * 0.8, 0.25, [c[0], c[1], c[2], 0.7]);
    }
}

fn effect_system(world: &World, dt: f32) {
    let mut effects = world.write::<Effect>();
    for (e, fx) in effects.iter_mut() {
        fx.ttl -= dt;
        if fx.ttl <= 0.0 {
            world.despawn(e);
        }
    }
}

fn game_over_system(world: &World) {
    let mut run = world.res_mut::<Run>();
    if run.hp > 0.0 {
        return;
    }
    run.hp = 0.0;
    run.phase = Phase::GameOver;
    run.result = Some(RunResult { time: run.time, kills: run.kills, coins: run.coins });
    let mut p = world.res_mut::<Persist>();
    if run.time > p.data.best_time {
        p.data.best_time = run.time;
    }
    p.dirty = true;
}
