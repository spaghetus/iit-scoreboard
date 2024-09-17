use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct LeaderboardEntry {
    pub location: String,
    pub fire_alarms: u32,
    pub entrapments: u32,
}
