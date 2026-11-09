//! Wire protocol between the Chroma Siege client and the game server.
//! JSON over WebSocket, every message is an object with a `type` tag.

use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum Hue {
    Red,
    Green,
    Blue,
}

impl Hue {
    pub const ALL: [Hue; 3] = [Hue::Red, Hue::Green, Hue::Blue];
    pub fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum GunKind {
    Direct,
    Mortar,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum BlockKind {
    /// Harmless, blocks direct fire, never targeted on purpose.
    Armored,
    /// Attacks the arsenal from the strip, targeted by guns of the same hue.
    Attacker(Hue),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum Stat {
    Slots,
    Damage,
    Health,
    Regen,
}

impl Stat {
    pub const ALL: [Stat; 4] = [Stat::Slots, Stat::Damage, Stat::Health, Stat::Regen];
    pub fn index(self) -> usize {
        self as usize
    }
}

/// What dealt the killing blow.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum KillSource {
    Direct,
    Mortar,
    Strip,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum GameEvent {
    WaveStarted { wave: u32, level: f32 },
    Spawned { block: BlockKind },
    Shot { gun: GunKind, hue: Hue },
    Hit { gun: GunKind, block: BlockKind },
    Killed { block: BlockKind, by: KillSource },
    GunPicked { gun: GunKind, hue: Hue, stack: u8 },
    Upgraded { stat: Stat },
    ArsenalHit { damage: f32 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMsg {
    /// Starts (or restarts) a run; the server drops any previous run state.
    StartRun { version: u32 },
    Events { events: Vec<GameEvent> },
    Claim { quest_id: u32 },
    EndRun,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reward {
    pub cash: u64,
    pub coins: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum QuestKind {
    KillHue { hue: Hue },
    Shots,
    Hits,
    DirectKills,
    MortarKills,
    ArmoredKills,
    StripKills,
    GunPicks,
    Upgrades,
    CleanWaves,
}

impl QuestKind {
    pub fn describe(&self, target: u32) -> String {
        match self {
            QuestKind::KillHue { hue } => {
                let name = match hue {
                    Hue::Red => "красных",
                    Hue::Green => "зелёных",
                    Hue::Blue => "синих",
                };
                format!("Уничтожь {target} {name} блоков")
            }
            QuestKind::Shots => format!("Сделай {target} выстрелов"),
            QuestKind::Hits => format!("Попади по блокам {target} раз"),
            QuestKind::DirectKills => format!("Уничтожь {target} блоков прямой наводкой"),
            QuestKind::MortarKills => format!("Уничтожь {target} блоков мортирой"),
            QuestKind::ArmoredKills => format!("Разрушь {target} серых блоков"),
            QuestKind::StripKills => format!("Добей полосой {target} блоков"),
            QuestKind::GunPicks => format!("Возьми {target} пушек из стеков"),
            QuestKind::Upgrades => format!("Купи {target} улучшений в бою"),
            QuestKind::CleanWaves => format!("Продержись {target} волн подряд без урона"),
        }
    }
}

/// Unix-millisecond timestamps are the only thing the server remembers about a quest
/// besides its live progress.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QuestView {
    pub id: u32,
    pub kind: QuestKind,
    pub target: u32,
    pub progress: u32,
    pub reward: Reward,
    pub created_at: u64,
    pub completed_at: Option<u64>,
    pub claimed_at: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub waves: u32,
    pub spawned: u32,
    pub shots_direct: u32,
    pub shots_mortar: u32,
    pub hits: u32,
    /// Indexed by `Hue::index()`.
    pub kills_by_hue: [u32; 3],
    pub kills_armored: u32,
    pub kills_direct: u32,
    pub kills_mortar: u32,
    pub kills_strip: u32,
    pub picks: u32,
    pub upgrades: u32,
    pub damage_taken: f32,
    pub quests_completed: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMsg {
    RunStarted { started_at: u64 },
    /// Periodic snapshot; `quests` holds every quest not yet claimed.
    Stats { stats: Stats, quests: Vec<QuestView> },
    QuestOffered { quest: QuestView },
    QuestCompleted { quest: QuestView },
    RewardGranted { quest_id: u32, reward: Reward },
    Error { message: String },
}
