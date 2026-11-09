use crate::math::Vec2;
use serde::{Deserialize, Serialize};

pub use chroma_protocol::{BlockKind, GunKind, Hue, KillSource, Stat};

#[derive(Clone, Copy, Debug)]
pub struct GunSpec {
    pub kind: GunKind,
    pub hue: Hue,
}

// ---------- components ----------

pub struct Pos(pub Vec2);

pub struct Block {
    pub kind: BlockKind,
    pub col: usize,
    /// Enemy level at spawn time; scales damage and rewards.
    pub power: f32,
}

pub struct Health {
    pub hp: f32,
    pub max: f32,
}

pub struct AttackTimer(pub f32);

/// Set on the block by whatever brought its HP to zero first.
pub struct Killer(pub KillSource);

pub struct Gun {
    pub spec: GunSpec,
    pub shots_left: u32,
    pub shots_max: u32,
    pub cooldown: f32,
    pub slot: usize,
    pub aim: Vec2,
}

pub struct Bullet {
    pub vel: Vec2,
    pub hue: Hue,
    pub dmg: f32,
}

pub struct Shell {
    pub from: Vec2,
    pub to: Vec2,
    pub t: f32,
    pub hue: Hue,
    pub dmg: f32,
}

pub struct Effect {
    pub ttl: f32,
    pub max_ttl: f32,
    pub r0: f32,
    pub r1: f32,
    pub color: [f32; 4],
}

// ---------- upgrades ----------

#[derive(Clone, Copy, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct Levels {
    pub slots: u32,
    pub damage: u32,
    pub health: u32,
    pub regen: u32,
}

impl Levels {
    pub fn get(&self, s: Stat) -> u32 {
        match s {
            Stat::Slots => self.slots,
            Stat::Damage => self.damage,
            Stat::Health => self.health,
            Stat::Regen => self.regen,
        }
    }
    pub fn inc(&mut self, s: Stat) {
        match s {
            Stat::Slots => self.slots += 1,
            Stat::Damage => self.damage += 1,
            Stat::Health => self.health += 1,
            Stat::Regen => self.regen += 1,
        }
    }
    pub fn plus(&self, o: Levels) -> Levels {
        Levels {
            slots: self.slots + o.slots,
            damage: self.damage + o.damage,
            health: self.health + o.health,
            regen: self.regen + o.regen,
        }
    }
}
