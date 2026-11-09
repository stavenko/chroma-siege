//! Layout and balance constants. Logical canvas is W x H, y grows downward.

use crate::components::Stat;

pub const W: f32 = 600.0;
pub const H: f32 = 800.0;
pub const COLS: usize = 10;
pub const CELL: f32 = W / COLS as f32;
pub const BLOCK: f32 = 52.0;

pub const STRIP_TOP: f32 = 520.0;
pub const STRIP_BOTTOM: f32 = 600.0;
pub const ARSENAL_TOP: f32 = 600.0;
pub const ARSENAL_Y: f32 = 636.0;
pub const ARSENAL_BOTTOM: f32 = 694.0;
pub const HP_BAR_Y: f32 = 682.0;
pub const STACK_Y: f32 = 748.0;
pub const STACK_W: f32 = 150.0;
pub const STACK_H: f32 = 84.0;
pub const STACK_XS: [f32; 3] = [110.0, 300.0, 490.0];

pub fn slot_x(slot: usize, slots: usize) -> f32 {
    (slot as f32 + 0.5) * W / slots as f32
}

// ---------- enemies ----------
pub const BLOCK_SPEED: f32 = 14.0;
pub const ROW_INTERVAL: f32 = CELL / BLOCK_SPEED;
pub const SPAWN_Y: f32 = -CELL / 2.0;
pub const PRESPAWN_ROWS: usize = 4;
/// A wave is WAVE_ROWS_MIN..=WAVE_ROWS_MAX rows followed by a gap of WAVE_GAP_ROWS empty rows.
pub const WAVE_ROWS_MIN: u32 = 5;
pub const WAVE_ROWS_MAX: u32 = 7;
pub const WAVE_GAP_ROWS: f32 = 2.0;
pub const FILL_PROB: f32 = 0.5;
pub const ARMORED_PROB: f32 = 0.25;

/// Enemy level grows as a smooth exponent: doubles every LEVEL_DOUBLING_SECS.
pub const LEVEL_DOUBLING_SECS: f32 = 150.0;
pub fn enemy_level(t: f32) -> f32 {
    (t * std::f32::consts::LN_2 / LEVEL_DOUBLING_SECS).exp()
}

pub const ATTACKER_HP: f32 = 30.0;
pub const ARMORED_HP: f32 = 110.0;
pub const ATTACKER_DMG: f32 = 4.0;
pub const ATTACK_INTERVAL: f32 = 1.5;
pub const STRIP_DPS: f32 = 6.0;

pub const ATTACKER_CASH: f32 = 4.0;
pub const ARMORED_CASH: f32 = 2.0;
pub const CASH_LEVEL_EXP: f32 = 0.9;
pub const ATTACKER_COIN_CHANCE: f32 = 0.12;
pub const ARMORED_COIN_CHANCE: f32 = 0.3;

// ---------- guns ----------
pub struct GunParams {
    pub dmg: f32,
    pub cooldown: f32,
    pub shots: u32,
}
pub const DIRECT: GunParams = GunParams { dmg: 10.0, cooldown: 0.45, shots: 30 };
pub const MORTAR: GunParams = GunParams { dmg: 22.0, cooldown: 1.3, shots: 10 };
pub const MORTAR_CHANCE: f32 = 0.3;
pub const BULLET_SPEED: f32 = 700.0;
pub const BULLET_R: f32 = 4.0;
pub const MUZZLE: f32 = 26.0;
pub const SHELL_FLIGHT: f32 = 0.9;
pub const SHELL_ARC: f32 = 140.0;
pub const MORTAR_RADIUS: f32 = 65.0;

// ---------- arsenal / upgrades ----------
pub const BASE_SLOTS: usize = 3;
pub const MAX_SLOTS: usize = 8;
pub const BASE_HP: f32 = 100.0;
pub const HP_GROWTH: f32 = 1.2;
pub const DMG_GROWTH: f32 = 1.15;
pub const REGEN_STEP: f32 = 0.6;
pub const REGEN_GROWTH: f32 = 1.12;

pub fn slots_for(level: u32) -> usize {
    (BASE_SLOTS + level as usize).min(MAX_SLOTS)
}
pub fn dmg_mult_for(level: u32) -> f32 {
    DMG_GROWTH.powi(level as i32)
}
pub fn max_hp_for(level: u32) -> f32 {
    BASE_HP * HP_GROWTH.powi(level as i32)
}
pub fn regen_for(level: u32) -> f32 {
    REGEN_STEP * level as f32 * REGEN_GROWTH.powi(level as i32)
}

const RUN_COST_BASE: [f32; 4] = [60.0, 25.0, 20.0, 30.0];
const RUN_COST_GROWTH: f32 = 1.35;
const PERM_COST_BASE: [f32; 4] = [15.0, 5.0, 5.0, 6.0];
const PERM_COST_GROWTH: f32 = 1.6;

/// In-battle cost in cash, grows with the number of in-battle purchases of this stat.
pub fn run_cost(stat: Stat, run_level: u32) -> u64 {
    (RUN_COST_BASE[stat.index()] * RUN_COST_GROWTH.powi(run_level as i32)).round() as u64
}

/// Permanent cost in coins, grows with the permanent level of this stat.
pub fn perm_cost(stat: Stat, perm_level: u32) -> u64 {
    (PERM_COST_BASE[stat.index()] * PERM_COST_GROWTH.powi(perm_level as i32)).round() as u64
}

pub fn is_maxed(stat: Stat, total_level: u32) -> bool {
    stat == Stat::Slots && slots_for(total_level) >= MAX_SLOTS
}
