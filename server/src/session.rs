//! Per-connection run state. Lives only in memory for the lifetime of the socket:
//! live counters plus, for every quest, the timestamps of when it was offered,
//! completed and claimed.

use chroma_protocol::*;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).expect("clock before 1970").as_millis() as u64
}

/// Quests are offered every QUEST_EVERY_MIN..=QUEST_EVERY_MAX waves.
pub const QUEST_EVERY_MIN: u32 = 5;
pub const QUEST_EVERY_MAX: u32 = 10;
const COIN_CHANCE: f32 = 0.3;
const COINS_MIN: u64 = 1;
const COINS_MAX: u64 = 3;
/// Each next quest asks for TARGET_GROWTH more than the previous one.
const TARGET_GROWTH: f32 = 0.25;
const CASH_LEVEL_EXP: f32 = 0.9;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn range(&mut self, lo: u64, hi_inclusive: u64) -> u64 {
        lo + self.next() % (hi_inclusive - lo + 1)
    }
    fn chance(&mut self, p: f32) -> bool {
        ((self.next() >> 40) as f32 / (1u64 << 24) as f32) < p
    }
}

/// (base target, base cash) for each quest type.
fn quest_base(kind: QuestKind) -> (u32, f32) {
    match kind {
        QuestKind::KillHue { .. } => (15, 40.0),
        QuestKind::Shots => (150, 30.0),
        QuestKind::Hits => (100, 35.0),
        QuestKind::DirectKills => (20, 40.0),
        QuestKind::MortarKills => (10, 50.0),
        QuestKind::ArmoredKills => (6, 45.0),
        QuestKind::StripKills => (8, 40.0),
        QuestKind::GunPicks => (8, 25.0),
        QuestKind::Upgrades => (3, 30.0),
        QuestKind::CleanWaves => (2, 70.0),
    }
}

/// How much one event advances a quest; `None` resets progress to zero.
fn quest_delta(kind: QuestKind, e: &GameEvent) -> Option<u32> {
    let d = match (kind, e) {
        (QuestKind::KillHue { hue }, GameEvent::Killed { block: BlockKind::Attacker(h), .. }) if *h == hue => 1,
        (QuestKind::Shots, GameEvent::Shot { .. }) => 1,
        (QuestKind::Hits, GameEvent::Hit { .. }) => 1,
        (QuestKind::DirectKills, GameEvent::Killed { by: KillSource::Direct, .. }) => 1,
        (QuestKind::MortarKills, GameEvent::Killed { by: KillSource::Mortar, .. }) => 1,
        (QuestKind::ArmoredKills, GameEvent::Killed { block: BlockKind::Armored, .. }) => 1,
        (QuestKind::StripKills, GameEvent::Killed { by: KillSource::Strip, .. }) => 1,
        (QuestKind::GunPicks, GameEvent::GunPicked { .. }) => 1,
        (QuestKind::Upgrades, GameEvent::Upgraded { .. }) => 1,
        (QuestKind::CleanWaves, GameEvent::WaveStarted { .. }) => 1,
        (QuestKind::CleanWaves, GameEvent::ArsenalHit { .. }) => return None,
        _ => 0,
    };
    Some(d)
}

pub struct Session {
    pub started_at: u64,
    pub stats: Stats,
    quests: Vec<QuestView>,
    next_id: u32,
    level: f32,
    next_quest_wave: u32,
    issued: u32,
    rng: Rng,
    pub dirty: bool,
}

impl Session {
    pub fn new(seed: u64) -> Self {
        let mut rng = Rng(seed | 1);
        let next_quest_wave = rng.range(QUEST_EVERY_MIN as u64, QUEST_EVERY_MAX as u64) as u32;
        Session {
            started_at: now_ms(),
            stats: Stats::default(),
            quests: Vec::new(),
            next_id: 1,
            level: 1.0,
            next_quest_wave,
            issued: 0,
            rng,
            dirty: true,
        }
    }

    pub fn snapshot(&self) -> ServerMsg {
        ServerMsg::Stats {
            stats: self.stats.clone(),
            quests: self.quests.iter().filter(|q| q.claimed_at.is_none()).cloned().collect(),
        }
    }

    /// Applies events in order. After every counter update all open quests are checked.
    pub fn apply(&mut self, events: &[GameEvent]) -> Vec<ServerMsg> {
        let mut out = Vec::new();
        for e in events {
            self.count(e);
            if let GameEvent::WaveStarted { wave, level } = e {
                self.level = *level;
                if *wave >= self.next_quest_wave {
                    self.next_quest_wave =
                        wave + self.rng.range(QUEST_EVERY_MIN as u64, QUEST_EVERY_MAX as u64) as u32;
                    out.push(ServerMsg::QuestOffered { quest: self.offer() });
                }
            }
            for q in self.quests.iter_mut().filter(|q| q.completed_at.is_none()) {
                match quest_delta(q.kind, e) {
                    Some(d) => q.progress += d,
                    None => q.progress = 0,
                }
                if q.progress >= q.target {
                    q.progress = q.target;
                    q.completed_at = Some(now_ms());
                    self.stats.quests_completed += 1;
                    out.push(ServerMsg::QuestCompleted { quest: q.clone() });
                }
            }
        }
        self.dirty = true;
        out
    }

    fn count(&mut self, e: &GameEvent) {
        let s = &mut self.stats;
        match e {
            GameEvent::WaveStarted { wave, .. } => s.waves = s.waves.max(*wave),
            GameEvent::Spawned { .. } => s.spawned += 1,
            GameEvent::Shot { gun: GunKind::Direct, .. } => s.shots_direct += 1,
            GameEvent::Shot { gun: GunKind::Mortar, .. } => s.shots_mortar += 1,
            GameEvent::Hit { .. } => s.hits += 1,
            GameEvent::Killed { block, by } => {
                match block {
                    BlockKind::Attacker(h) => s.kills_by_hue[h.index()] += 1,
                    BlockKind::Armored => s.kills_armored += 1,
                }
                match by {
                    KillSource::Direct => s.kills_direct += 1,
                    KillSource::Mortar => s.kills_mortar += 1,
                    KillSource::Strip => s.kills_strip += 1,
                }
            }
            GameEvent::GunPicked { .. } => s.picks += 1,
            GameEvent::Upgraded { .. } => s.upgrades += 1,
            GameEvent::ArsenalHit { damage } => s.damage_taken += damage,
        }
    }

    fn offer(&mut self) -> QuestView {
        const KINDS: [QuestKind; 10] = [
            QuestKind::KillHue { hue: Hue::Red },
            QuestKind::Shots,
            QuestKind::Hits,
            QuestKind::DirectKills,
            QuestKind::MortarKills,
            QuestKind::ArmoredKills,
            QuestKind::StripKills,
            QuestKind::GunPicks,
            QuestKind::Upgrades,
            QuestKind::CleanWaves,
        ];
        let open: Vec<QuestKind> =
            self.quests.iter().filter(|q| q.completed_at.is_none()).map(|q| q.kind).collect();
        // Prefer a type that is not already open.
        let mut kind = KINDS[self.rng.range(0, 9) as usize];
        for _ in 0..10 {
            if !open.iter().any(|k| std::mem::discriminant(k) == std::mem::discriminant(&kind)) {
                break;
            }
            kind = KINDS[self.rng.range(0, 9) as usize];
        }
        if let QuestKind::KillHue { .. } = kind {
            kind = QuestKind::KillHue { hue: Hue::ALL[self.rng.range(0, 2) as usize] };
        }
        let (base_target, base_cash) = quest_base(kind);
        let growth = 1.0 + TARGET_GROWTH * self.issued as f32;
        let target = (base_target as f32 * growth).round() as u32;
        let cash = (base_cash * growth * self.level.powf(CASH_LEVEL_EXP)).round() as u64;
        let coins = if self.rng.chance(COIN_CHANCE) { self.rng.range(COINS_MIN, COINS_MAX) } else { 0 };
        let quest = QuestView {
            id: self.next_id,
            kind,
            target,
            progress: 0,
            reward: Reward { cash, coins },
            created_at: now_ms(),
            completed_at: None,
            claimed_at: None,
        };
        self.next_id += 1;
        self.issued += 1;
        self.quests.push(quest.clone());
        quest
    }

    pub fn claim(&mut self, quest_id: u32) -> Result<Reward, String> {
        let q = self
            .quests
            .iter_mut()
            .find(|q| q.id == quest_id)
            .ok_or_else(|| format!("quest {quest_id} does not exist"))?;
        if q.completed_at.is_none() {
            return Err(format!("quest {quest_id} is not completed ({}/{})", q.progress, q.target));
        }
        if q.claimed_at.is_some() {
            return Err(format!("quest {quest_id} is already claimed"));
        }
        q.claimed_at = Some(now_ms());
        self.dirty = true;
        Ok(q.reward)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wave(n: u32) -> GameEvent {
        GameEvent::WaveStarted { wave: n, level: 1.0 }
    }

    fn session_with_quest(kind: QuestKind, target: u32) -> Session {
        let mut s = Session::new(42);
        s.quests.push(QuestView {
            id: 99,
            kind,
            target,
            progress: 0,
            reward: Reward { cash: 10, coins: 1 },
            created_at: now_ms(),
            completed_at: None,
            claimed_at: None,
        });
        s
    }

    #[test]
    fn quest_offered_every_5_to_10_waves() {
        let mut s = Session::new(7);
        let mut offered_at = Vec::new();
        for w in 1..=100 {
            if s.apply(&[wave(w)]).iter().any(|m| matches!(m, ServerMsg::QuestOffered { .. })) {
                offered_at.push(w);
            }
        }
        assert!(offered_at[0] >= QUEST_EVERY_MIN && offered_at[0] <= QUEST_EVERY_MAX, "{offered_at:?}");
        for pair in offered_at.windows(2) {
            let gap = pair[1] - pair[0];
            assert!((QUEST_EVERY_MIN..=QUEST_EVERY_MAX).contains(&gap), "{offered_at:?}");
        }
    }

    #[test]
    fn kill_hue_counts_only_its_hue_and_completes() {
        let mut s = session_with_quest(QuestKind::KillHue { hue: Hue::Red }, 2);
        let red = GameEvent::Killed { block: BlockKind::Attacker(Hue::Red), by: KillSource::Direct };
        let blue = GameEvent::Killed { block: BlockKind::Attacker(Hue::Blue), by: KillSource::Direct };
        assert!(s.apply(&[red.clone(), blue]).is_empty());
        let out = s.apply(&[red]);
        assert!(matches!(&out[..], [ServerMsg::QuestCompleted { quest }] if quest.id == 99 && quest.completed_at.is_some()));
        assert_eq!(s.stats.kills_by_hue, [2, 0, 1]);
    }

    #[test]
    fn clean_waves_reset_on_damage() {
        let mut s = session_with_quest(QuestKind::CleanWaves, 2);
        s.next_quest_wave = u32::MAX;
        s.apply(&[wave(1), GameEvent::ArsenalHit { damage: 3.0 }, wave(2)]);
        assert_eq!(s.quests[0].progress, 1);
        let out = s.apply(&[wave(3)]);
        assert!(matches!(&out[..], [ServerMsg::QuestCompleted { .. }]));
    }

    #[test]
    fn claim_rules() {
        let mut s = session_with_quest(QuestKind::Shots, 1);
        assert!(s.claim(99).unwrap_err().contains("not completed"));
        s.apply(&[GameEvent::Shot { gun: GunKind::Direct, hue: Hue::Green }]);
        assert_eq!(s.claim(99).unwrap(), Reward { cash: 10, coins: 1 });
        assert!(s.claim(99).unwrap_err().contains("already claimed"));
        assert!(s.claim(5).unwrap_err().contains("does not exist"));
        match s.snapshot() {
            ServerMsg::Stats { quests, .. } => assert!(quests.is_empty()),
            other => panic!("unexpected {other:?}"),
        }
    }
}
