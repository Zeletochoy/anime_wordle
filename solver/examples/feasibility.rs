//! Full-DB feasibility check for the exact search.
//!
//! Build first, then measure the binary alone (peak RSS in "maximum resident set size"):
//! `cargo build --release --example feasibility && /usr/bin/time -l ./target/release/examples/feasibility`

use anime_wordle_solver::{
    data,
    feedback::Table,
    solver::{Probes, Solver},
};
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let db = data::load(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../src/data/characters.json"
    ))?;
    let table = Table::new(&db);
    let all: Vec<u16> = (0..db.len() as u16).collect();
    let mut solver = Solver::new(&table, &db, Probes::Free);
    let start = Instant::now();
    let total = solver
        .total_cost(&all)
        .ok_or_else(|| anyhow::anyhow!("no winning strategy"))?;
    let opener = solver.best_guess(&all).expect("solved above") as usize;
    let stats = solver.stats();
    println!("characters: {}", db.len());
    println!(
        "expected guesses: {:.6} ({total} / {})",
        total as f64 / db.len() as f64,
        db.len()
    );
    println!("opener: {} ({})", db[opener].name, db[opener].anime.name);
    println!(
        "states: {}, guesses evaluated: {}",
        stats.states, stats.guesses_evaluated
    );
    println!("wall time: {:.2?}", start.elapsed());
    Ok(())
}
