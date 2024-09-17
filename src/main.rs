use chrono::{naive::NaiveDateDaysIterator, Local, NaiveDate};
use clap::Parser;
use tracing::info;

#[derive(Parser)]
struct Args {
	from: NaiveDate,
	to: Option<NaiveDate>,
}

fn main() {
	tracing::subscriber::set_global_default(tracing_subscriber::FmtSubscriber::new()).unwrap();
	let Args { from, to } = Args::parse();
	let to = to.unwrap_or_else(|| Local::now().date_naive());
	info!("Processing articles from {from} to {to}");

	let scores = from.iter_days().take_while(|d| d < &to);
}
