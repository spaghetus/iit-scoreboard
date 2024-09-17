use std::io::stdin;

use iit_scoreboard::LeaderboardEntry;
use serde::Serialize;
use tera::{Context, Tera};

#[derive(Serialize)]
struct Entry {
    max_fire: bool,
    max_entrapment: bool,
    location: String,
    fire_alarms: u32,
    entrapments: u32,
}

fn main() {
    let tera = match Tera::new("templates/**/*.html") {
        Ok(t) => t,
        Err(e) => {
            println!("Parsing error(s): {}", e);
            ::std::process::exit(1);
        }
    };
    let mut reader = csv::Reader::from_reader(stdin());
    let leaderboard: Vec<LeaderboardEntry> = reader.deserialize().flatten().collect();
    let max_fire = leaderboard.iter().map(|l| l.fire_alarms).max().unwrap_or(0);
    let max_entrapments = leaderboard.iter().map(|l| l.entrapments).max().unwrap_or(0);
    let entries: Vec<_> = leaderboard
        .into_iter()
        .map(
            |LeaderboardEntry {
                 location,
                 fire_alarms,
                 entrapments,
             }| Entry {
                max_fire: fire_alarms == max_fire,
                max_entrapment: entrapments == max_entrapments,
                location,
                fire_alarms,
                entrapments,
            },
        )
        .collect();
    let mut ctx = Context::new();
    ctx.insert("entries", &entries);
    let output = tera.render("leaderboard.html", &ctx).unwrap();
    println!("{output}");
}
