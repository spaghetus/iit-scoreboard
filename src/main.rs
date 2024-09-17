use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{stdout, BufReader},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use chrono::{DateTime, Datelike, Local, NaiveDate};
use clap::Parser;
use iit_scoreboard::LeaderboardEntry;
use indicatif::{MultiProgress, ProgressBar};
use rayon::prelude::*;
use regex::{Regex, RegexBuilder};
use rss::Channel;
use serde::Serialize;
use tokio::{sync::Semaphore, task::JoinSet};
use tracing::{error, info, instrument, span, trace, Instrument, Level, Span};
use tracing_indicatif::IndicatifLayer;
use tracing_panic::panic_hook;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser)]
struct Args {
    from: NaiveDate,
    to: Option<NaiveDate>,
    #[arg(short, default_value = "4")]
    parallelism: usize,
    #[arg(short, default_value = "Info")]
    verbosity: Level,
    #[arg(short)]
    file: Option<PathBuf>,
}

#[tokio::main]
async fn main() {
    main_().await
}

#[derive(Debug)]
pub struct Event {
    pub incident_type: String,
    pub location: String,
    pub notes: String,
}

#[instrument]
async fn main_() {
    let Args {
        from,
        to,
        parallelism,
        verbosity,
        file,
    } = Args::parse();
    let to = to.unwrap_or_else(|| Local::now().date_naive());
    let indicatif_layer = IndicatifLayer::new();

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_writer(indicatif_layer.get_stderr_writer()))
        .with(tracing_subscriber::filter::LevelFilter::from_level(
            verbosity,
        ))
        .with(indicatif_layer)
        .init();
    std::panic::set_hook(Box::new(panic_hook));
    info!("Processing events from {from} to {to}");

    let mut channel = file
        .and_then(|file| {
            Channel::read_from(BufReader::new(File::open(file).expect("Read input file"))).ok()
        })
        .unwrap_or_default();

    let from = if let Some(max) = channel
        .items
        .iter()
        .flat_map(|a| a.pub_date())
        .flat_map(|d| DateTime::parse_from_rfc2822(d).ok())
        .max()
    {
        from.max(max.date_naive().succ_opt().unwrap())
    } else {
        from
    };
    dbg!(&from);

    let mut set = JoinSet::new();
    let sema = Arc::new(Semaphore::new(parallelism));

    for date in from.iter_days().take_while(|d| d <= &to) {
        let sema = sema.clone();
        set.spawn(async move {
            let handle = sema.acquire().await.unwrap();
            let out = article(date).await;
            std::mem::drop(handle);
            out
        });
    }

    while let Some(finished) = set.join_next().await {
        let mut finished = finished.expect("Task panic");
        channel.items.append(&mut finished.items);
    }
    channel
        .items
        .sort_by(|left, right| left.pub_date().cmp(&right.pub_date()));

    info!("Saving channel to ./channel.rss for later");
    channel
        .clone()
        .pretty_write_to(
            OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open("./channel.rss")
                .expect("Open saved rss"),
            b'\t',
            1,
        )
        .expect("Save rss");
    info!("{} articles", channel.items.len());
    trace!("Parsing...");
    let incident_regex =
        RegexBuilder::new(r"incident type:([^\n]+)\n+location:([^\n]+)[\s\S]*?notes:([^\n]+)")
            .case_insensitive(true)
            .build()
            .unwrap();
    let alarm_regex = RegexBuilder::new("alarm")
        .case_insensitive(true)
        .build()
        .unwrap();
    let elevator_regex = RegexBuilder::new("entrapment")
        .case_insensitive(true)
        .build()
        .unwrap();
    let events: Vec<_> = channel
        .items
        .par_iter()
        .flat_map(|a| a.content.as_deref())
        .map(|a| html2text::from_read(a.as_bytes(), usize::MAX))
        .inspect(|a| trace!("{a}"))
        .flat_map(|a| {
            incident_regex
                .captures_iter(a.as_str())
                .map(|c| Event {
                    incident_type: c.get(1).unwrap().as_str().trim().to_string(),
                    location: c.get(2).unwrap().as_str().trim().to_string(),
                    notes: c.get(3).unwrap().as_str().trim().to_string(),
                })
                .collect::<Vec<_>>()
                .into_par_iter()
        })
        .collect();
    info!("{} events", events.len());
    let counts = events
        .iter()
        .map(|incident| {
            let location = incident
                .location
                .split(":")
                .last()
                .unwrap_or(&incident.location)
                .trim();
            let is_fire = alarm_regex.is_match(&incident.incident_type);
            let is_elevator = elevator_regex.is_match(&incident.incident_type);
            (location, is_fire, is_elevator)
        })
        .fold(HashMap::new(), |mut acc, (loc, is_fire, is_elevator)| {
            let (fire_count, elevator_count) = acc.entry(loc).or_insert((0, 0));
            *fire_count += is_fire as u32;
            *elevator_count += is_elevator as u32;
            acc
        });
    let mut leaderboard = counts
        .into_iter()
        .map(|(location, (fire_alarms, entrapments))| LeaderboardEntry {
            location: location.to_string(),
            fire_alarms,
            entrapments,
        })
        .collect::<Vec<_>>();
    leaderboard.sort_by_key(
        |LeaderboardEntry {
             location: _,
             fire_alarms,
             entrapments,
         }| fire_alarms + entrapments,
    );
    leaderboard.reverse();
    for LeaderboardEntry {
        location,
        fire_alarms,
        entrapments,
    } in &leaderboard[..10]
    {
        info!("{location}: {fire_alarms} fire alarms, {entrapments} entrapments");
    }
    let mut writer = csv::Writer::from_writer(stdout());
    for element in leaderboard {
        writer.serialize(element).unwrap();
    }
}

#[instrument]
async fn article(date: NaiveDate) -> Channel {
    let url = format!(
        "https://blogs.iit.edu/public_safety/{:04}/{:02}/{:02}/feed/",
        date.year(),
        date.month0() + 1,
        date.day0() + 1
    );
    let request = reqwest::get(&url)
        .instrument(span!(Level::INFO, "HTTP", url = &url))
        .await
        .expect("HTTP request");
    let body = request.bytes().await.expect("Byte body");
    Channel::read_from(&body[..]).expect("RSS feed")
}
