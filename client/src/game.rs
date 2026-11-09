//! Resources, actions and systems.

use crate::components::*;
use crate::config::*;
use crate::ecs::{Entity, World};
use crate::math::Vec2;
use crate::save::SaveData;
use chroma_protocol::{ClientMsg, GameEvent, QuestView, ServerMsg, Stats, PROTOCOL_VERSION};

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
    ClaimQuest(u32),
}

/// Connection-level notifications coming from the network layer.
pub enum NetEvent {
    Open,
    Message(ServerMsg),
    Closed(String),
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
    pub fn range_u32(&mut self, lo: u32, hi_inclusive: u32) -> u32 {
        lo + (self.next_u64() % (hi_inclusive - lo + 1) as u64) as u32
    }
}

pub struct Persist {
    pub data: SaveData,
    pub dirty: bool,
}

/// Messages for the server. Events are batched; a control message flushes
/// pending events first so ordering is preserved.
#[derive(Default)]
pub struct Outbox {
    queue: Vec<ClientMsg>,
    events: Vec<GameEvent>,
}

impl Outbox {
    pub fn event(&mut self, e: GameEvent) {
        self.events.push(e);
    }
    pub fn send(&mut self, msg: ClientMsg) {
        self.batch_events();
        self.queue.push(msg);
    }
    fn batch_events(&mut self) {
        if !self.events.is_empty() {
            let events = std::mem::take(&mut self.events);
            self.queue.push(ClientMsg::Events { events });
        }
    }
    pub fn drain(&mut self) -> Vec<ClientMsg> {
        self.batch_events();
        std::mem::take(&mut self.queue)
    }
    pub fn clear(&mut self) {
        self.queue.clear();
        self.events.clear();
    }
}

#[derive(Default)]
pub struct Link {
    pub connected: bool,
    pub last_error: Option<String>,
}

/// Server-side view of the current run, as last reported by the server.
#[derive(Default)]
pub struct Quests {
    pub list: Vec<QuestView>,
    pub stats: Option<Stats>,
    pub pending_claims: Vec<u32>,
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
    pub wave: u32,
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
    pub wave: u32,
    /// Rows still to spawn in the current wave.
    pub rows_left: u32,
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
            wave: 0,
            rows_left: 0,
            result: None,
        }
    }
    pub fn in_battle(&self) -> bool {
        matches!(self.phase, Phase::Playing | Phase::Paused)
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

fn emit(world: &World, e: GameEvent) {
    world.res_mut::<Outbox>().event(e);
}

// ---------- world setup ----------

pub fn create_world(save: SaveData, seed: u64) -> World {
    let mut w = World::new();
    w.register::<Pos>();
    w.register::<Block>();
    w.register::<Health>();
    w.register::<AttackTimer>();
    w.register::<Killer>();
    w.register::<Gun>();
    w.register::<Bullet>();
    w.register::<Shell>();
    w.register::<Effect>();
    w.insert_resource(Rng::new(seed));
    w.insert_resource(Persist { data: save, dirty: false });
    w.insert_resource(Run::new(Phase::Menu));
    w.insert_resource(Deck::empty());
    w.insert_resource(Outbox::default());
    w.insert_resource(Link::default());
    w.insert_resource(Quests::default());
    w
}

fn start_run(world: &World) {
    world.clear_entities();
    *world.res_mut::<Run>() = Run::new(Phase::Playing);
    *world.res_mut::<Quests>() = Quests::default();
    world.res_mut::<Outbox>().send(ClientMsg::StartRun { version: PROTOCOL_VERSION });
    let max_hp = arsenal_stats(world).max_hp;
    let first_wave_rows = {
        let mut rng = world.res_mut::<Rng>();
        *world.res_mut::<Deck>() = Deck::new(&mut rng);
        rng.range_u32(WAVE_ROWS_MIN, WAVE_ROWS_MAX)
    };
    {
        let mut run = world.res_mut::<Run>();
        run.hp = max_hp;
        run.wave = 1;
        run.rows_left = first_wave_rows - PRESPAWN_ROWS as u32;
    }
    emit(world, GameEvent::WaveStarted { wave: 1, level: 1.0 });
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
        emit(world, GameEvent::Spawned { block: kind });
    }
}

fn spawn_effect(world: &World, p: Vec2, r0: f32, r1: f32, ttl: f32, color: [f32; 4]) {
    let e = world.spawn();
    world.insert(e, Pos(p));
    world.insert(e, Effect { ttl, max_ttl: ttl, r0, r1, color });
}

// ---------- network ----------

pub fn handle_net(world: &World, ev: NetEvent) {
    match ev {
        NetEvent::Open => {
            {
                let mut link = world.res_mut::<Link>();
                link.connected = true;
                link.last_error = None;
            }
            // Reconnected mid-battle: the server forgot the run, start a fresh server session.
            let (in_battle, wave, time) = {
                let run = world.res::<Run>();
                (run.in_battle(), run.wave, run.time)
            };
            if in_battle {
                *world.res_mut::<Quests>() = Quests::default();
                let mut out = world.res_mut::<Outbox>();
                out.send(ClientMsg::StartRun { version: PROTOCOL_VERSION });
                out.event(GameEvent::WaveStarted { wave, level: enemy_level(time) });
            }
        }
        NetEvent::Closed(reason) => {
            {
                let mut link = world.res_mut::<Link>();
                link.connected = false;
                link.last_error = Some(reason);
            }
            world.res_mut::<Quests>().pending_claims.clear();
            let mut run = world.res_mut::<Run>();
            if run.phase == Phase::Playing {
                run.phase = Phase::Paused;
            }
        }
        NetEvent::Message(msg) => handle_server(world, msg),
    }
}

fn upsert_quest(list: &mut Vec<QuestView>, q: QuestView) {
    match list.iter_mut().find(|x| x.id == q.id) {
        Some(x) => *x = q,
        None => list.push(q),
    }
}

fn handle_server(world: &World, msg: ServerMsg) {
    match msg {
        ServerMsg::RunStarted { .. } => {}
        ServerMsg::Stats { stats, quests } => {
            let mut q = world.res_mut::<Quests>();
            q.stats = Some(stats);
            q.list = quests;
        }
        ServerMsg::QuestOffered { quest } | ServerMsg::QuestCompleted { quest } => {
            upsert_quest(&mut world.res_mut::<Quests>().list, quest);
        }
        ServerMsg::RewardGranted { quest_id, reward } => {
            {
                let mut q = world.res_mut::<Quests>();
                q.list.retain(|x| x.id != quest_id);
                q.pending_claims.retain(|&id| id != quest_id);
            }
            {
                let mut run = world.res_mut::<Run>();
                run.cash += reward.cash;
                run.coins += reward.coins;
            }
            if reward.coins > 0 {
                let mut p = world.res_mut::<Persist>();
                p.data.coins += reward.coins;
                p.dirty = true;
            }
        }
        ServerMsg::Error { message } => panic!("game server reported an error: {message}"),
    }
}

// ---------- actions ----------

pub fn handle_action(world: &World, action: UiAction) {
    let phase = world.res::<Run>().phase;
    let connected = world.res::<Link>().connected;
    match action {
        UiAction::Click(p) if phase == Phase::Playing => click(world, p),
        UiAction::Click(_) => {}
        UiAction::Start if connected && matches!(phase, Phase::Menu | Phase::GameOver) => start_run(world),
        UiAction::Start => {}
        UiAction::ToMenu if phase == Phase::GameOver => {
            world.clear_entities();
            *world.res_mut::<Deck>() = Deck::empty();
            *world.res_mut::<Quests>() = Quests::default();
            world.res_mut::<Run>().phase = Phase::Menu;
            if connected {
                world.res_mut::<Outbox>().send(ClientMsg::EndRun);
            }
        }
        UiAction::ToMenu => {}
        UiAction::TogglePause => {
            let mut run = world.res_mut::<Run>();
            run.phase = match run.phase {
                Phase::Playing => Phase::Paused,
                Phase::Paused if connected => Phase::Playing,
                other => other,
            };
        }
        UiAction::RunUpgrade(stat) if phase == Phase::Playing => run_upgrade(world, stat),
        UiAction::RunUpgrade(_) => {}
        UiAction::PermUpgrade(stat) if phase == Phase::Menu => perm_upgrade(world, stat),
        UiAction::PermUpgrade(_) => {}
        UiAction::ClaimQuest(id) if connected && phase == Phase::Playing => {
            let mut q = world.res_mut::<Quests>();
            let claimable = q.list.iter().any(|x| x.id == id && x.completed_at.is_some());
            if claimable && !q.pending_claims.contains(&id) {
                q.pending_claims.push(id);
                world.res_mut::<Outbox>().send(ClientMsg::Claim { quest_id: id });
            }
        }
        UiAction::ClaimQuest(_) => {}
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
    emit(world, GameEvent::GunPicked { gun: spec.kind, hue: spec.hue, stack: stack as u8 });
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
    emit(world, GameEvent::Upgraded { stat });
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
    strip_system(world, dt);
    regen_system(world, dt, &stats);
    death_system(world);
    world.maintain();
    effect_system(world, dt);
    world.maintain();
    game_over_system(world);
}

/// Waves of WAVE_ROWS_MIN..=WAVE_ROWS_MAX rows, separated by a gap of WAVE_GAP_ROWS rows.
fn spawn_system(world: &World, dt: f32) {
    let new_wave = {
        let mut run = world.res_mut::<Run>();
        run.spawn_timer -= dt;
        if run.spawn_timer > 0.0 {
            return;
        }
        run.rows_left == 0
    };
    if new_wave {
        let rows = world.res_mut::<Rng>().range_u32(WAVE_ROWS_MIN, WAVE_ROWS_MAX);
        let (wave, level) = {
            let mut run = world.res_mut::<Run>();
            run.wave += 1;
            run.rows_left = rows;
            (run.wave, enemy_level(run.time))
        };
        emit(world, GameEvent::WaveStarted { wave, level });
    }
    let power = {
        let mut run = world.res_mut::<Run>();
        run.rows_left -= 1;
        run.spawn_timer += if run.rows_left == 0 { ROW_INTERVAL * (1.0 + WAVE_GAP_ROWS) } else { ROW_INTERVAL };
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
    let mut shots = Vec::new();
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
            shots.push(GameEvent::Shot { gun: g.spec.kind, hue: g.spec.hue });
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
    let mut out = world.res_mut::<Outbox>();
    for s in shots {
        out.event(s);
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

/// Applies damage and remembers the source of the blow that took HP to zero.
fn apply_damage(world: &World, hits: &[(Entity, f32)], source: KillSource) {
    let mut health = world.write::<Health>();
    let mut killers = world.write::<Killer>();
    for &(e, d) in hits {
        let h = health.get_mut(e).expect("damaged block without Health");
        let was_alive = h.hp > 0.0;
        h.hp -= d;
        if was_alive && h.hp <= 0.0 {
            killers.insert(e, Killer(source));
        }
    }
}

/// Direct fire: flies straight, passes through blocks of other hues,
/// stops at the first armored block or block of its own hue.
fn bullet_system(world: &World, dt: f32) {
    let blocks = block_snapshot(world);
    let mut hits = Vec::new();
    let mut hit_events = Vec::new();
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
                    hit_events.push(GameEvent::Hit { gun: GunKind::Direct, block: kind });
                    sparks.push(pp);
                    world.despawn(e);
                    break;
                }
            }
        }
    }
    apply_damage(world, &hits, KillSource::Direct);
    {
        let mut out = world.res_mut::<Outbox>();
        for h in hit_events {
            out.event(h);
        }
    }
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
    let mut hit_events = Vec::new();
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
                hit_events.push(GameEvent::Hit { gun: GunKind::Mortar, block: kind });
            }
        }
    }
    apply_damage(world, &hits, KillSource::Mortar);
    {
        let mut out = world.res_mut::<Outbox>();
        for h in hit_events {
            out.event(h);
        }
    }
    for (at, hue, _) in explosions {
        let c = crate::draw::hue_color(hue);
        spawn_effect(world, at, 10.0, MORTAR_RADIUS, 0.35, [c[0], c[1], c[2], 0.55]);
    }
}

/// The strip hurts every block on it by a share of its max HP; attackers on it hit the arsenal.
fn strip_system(world: &World, dt: f32) {
    let mut strip_hits = Vec::new();
    let mut arsenal_hits = Vec::new();
    {
        let blocks = world.read::<Block>();
        let pos = world.read::<Pos>();
        let health = world.read::<Health>();
        let mut timers = world.write::<AttackTimer>();
        for (e, b) in blocks.iter() {
            let p = pos.get(e).expect("block without Pos").0;
            if p.y < STRIP_TOP || p.y > STRIP_BOTTOM {
                continue;
            }
            let max = health.get(e).expect("block without Health").max;
            strip_hits.push((e, max * STRIP_MAX_HP_PER_SEC * dt));
            if let BlockKind::Attacker(_) = b.kind {
                let t = timers.get_mut(e).expect("attacker without AttackTimer");
                t.0 -= dt;
                if t.0 <= 0.0 {
                    t.0 += ATTACK_INTERVAL;
                    arsenal_hits.push((Vec2::new(p.x, STRIP_BOTTOM + 4.0), ATTACKER_DMG * b.power));
                }
            }
        }
    }
    apply_damage(world, &strip_hits, KillSource::Strip);
    for (p, dmg) in arsenal_hits {
        world.res_mut::<Run>().hp -= dmg;
        emit(world, GameEvent::ArsenalHit { damage: dmg });
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
    let dead: Vec<(Entity, Vec2, BlockKind, f32, KillSource)> = {
        let blocks = world.read::<Block>();
        let pos = world.read::<Pos>();
        let health = world.read::<Health>();
        let killers = world.read::<Killer>();
        blocks
            .iter()
            .filter(|(e, _)| health.get(*e).expect("block without Health").hp <= 0.0)
            .map(|(e, b)| {
                let by = killers.get(e).expect("dead block without Killer").0;
                (e, pos.get(e).expect("block without Pos").0, b.kind, b.power, by)
            })
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
        let mut out = world.res_mut::<Outbox>();
        for (e, p, kind, power, by) in dead {
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
            out.event(GameEvent::Killed { block: kind, by });
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
    run.result = Some(RunResult { time: run.time, kills: run.kills, coins: run.coins, wave: run.wave });
    let mut p = world.res_mut::<Persist>();
    if run.time > p.data.best_time {
        p.data.best_time = run.time;
    }
    p.dirty = true;
}
