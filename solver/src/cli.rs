use crate::certify;
use crate::data::{Character, DEFAULT_CHARACTERS_URL};
use crate::feedback::Table;
use crate::history::{self, HistoryRule, PastPick};
use crate::oracle::Oracle;
use crate::solver::{self, Probes, Set, Solver};
use crate::supabase::Supabase;
use anyhow::Context;
use chrono::NaiveDate;
use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Write;
use std::time::Instant;

/// Day the footer's "no repeats" rule was announced (changelog entry 2026-07-14).
const RULE_SINCE: &str = "2026-07-14";

#[derive(Parser)]
#[command(
    about = "Plays Anime Wordle with minimum expected guesses, using only what a player can see"
)]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Cmd,
    /// Character DB: file path or URL (defaults to the file the daily picker reads)
    #[arg(long, global = true, default_value = DEFAULT_CHARACTERS_URL)]
    pub characters: String,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// Play today's daily and print a JSON report
    Play(PlayArgs),
    /// Measure the strategy offline, and optionally replay past dailies with and without the history prior
    Bench(BenchArgs),
    /// Check past dailies against the footer's no-repeat windows
    HistoryCheck(HistoryCheckArgs),
    /// Prove the strategy is optimal: an independent checker verifies the solver's results
    Certify,
}

#[derive(Args)]
pub struct RuleArgs {
    /// A character picked fewer than this many days ago is excluded by the history prior
    #[arg(long, default_value_t = 90)]
    pub character_days: i64,
    /// An anime picked fewer than this many days ago is excluded by the history prior
    #[arg(long, default_value_t = 30)]
    pub anime_days: i64,
}

impl RuleArgs {
    fn rule(&self) -> HistoryRule {
        HistoryRule {
            character_days: self.character_days,
            anime_days: self.anime_days,
        }
    }
}

#[derive(Args)]
pub struct PlayArgs {
    /// Exclude recently picked characters/anime (experimental, based on the footer's rule)
    #[arg(long)]
    pub history_prior: bool,
    #[command(flatten)]
    pub rule: RuleArgs,
    /// Submit the score to the public leaderboard
    #[arg(long, requires = "username")]
    pub submit: bool,
    /// Leaderboard name; must contain "bot" so the entry is clearly marked
    #[arg(long, value_parser = bot_username)]
    pub username: Option<String>,
}

#[derive(Args)]
pub struct BenchArgs {
    /// Also replay past dailies with and without the history prior (needs Supabase)
    #[arg(long)]
    pub history_prior: bool,
    /// First past day to replay
    #[arg(long, default_value = RULE_SINCE)]
    pub since: NaiveDate,
    #[command(flatten)]
    pub rule: RuleArgs,
}

#[derive(Args)]
pub struct HistoryCheckArgs {
    /// Only consider picks from this day on
    #[arg(long, default_value = RULE_SINCE)]
    pub since: NaiveDate,
    #[command(flatten)]
    pub rule: RuleArgs,
}

fn bot_username(s: &str) -> Result<String, String> {
    if s.to_lowercase().contains("bot") {
        Ok(s.trim().to_owned())
    } else {
        Err("must contain \"bot\" so the leaderboard entry is clearly marked".to_owned())
    }
}

#[derive(Serialize)]
pub struct GuessReport {
    pub name: String,
    pub anime: String,
}

#[derive(Serialize)]
pub struct PlayReport {
    pub day: NaiveDate,
    pub guesses: Vec<GuessReport>,
    pub count: usize,
    pub history_prior: bool,
    pub prior_violated: bool,
    pub submitted: bool,
    pub submit_error: Option<String>,
}

fn all(db: &[Character]) -> Set {
    (0..db.len() as u16).collect()
}

/// Past picks (day < today) resolved against the DB; names missing from the DB are reported on stderr.
fn past_picks(
    db: &[Character],
    supabase: &Supabase,
    today: NaiveDate,
) -> anyhow::Result<(Vec<PastPick>, usize)> {
    let rows = supabase.past_picks(today)?;
    let (picks, unknown) = history::resolve(db, &rows);
    if !unknown.is_empty() {
        eprintln!(
            "history: {} past picks are not in the character DB: {}",
            unknown.len(),
            unknown.join(", ")
        );
    }
    Ok((picks, unknown.len()))
}

pub fn run_play(
    db: &[Character],
    supabase: &Supabase,
    today: NaiveDate,
    args: &PlayArgs,
) -> anyhow::Result<PlayReport> {
    let table = Table::new(db);
    let mut solver = Solver::new(
        &table,
        db,
        if args.history_prior {
            Probes::Safe
        } else {
            Probes::Free
        },
    );
    let support = if args.history_prior {
        history::support(
            db,
            &past_picks(db, supabase, today)?.0,
            today,
            &args.rule.rule(),
        )
    } else {
        all(db)
    };
    let outcome = solver::play(
        &mut solver,
        support,
        &mut Oracle::daily(&table, db, supabase, today)?,
    )?;
    let count = outcome.guesses.len();
    let (submitted, submit_error) = match (args.submit, &args.username) {
        (true, Some(username)) => match supabase.submit_score(today, username, count) {
            Ok(()) => (true, None),
            Err(e) => (false, Some(format!("{e:#}"))),
        },
        _ => (false, None),
    };
    Ok(PlayReport {
        day: today,
        guesses: outcome
            .guesses
            .iter()
            .map(|&g| GuessReport {
                name: db[g].name.clone(),
                anime: db[g].anime.name.clone(),
            })
            .collect(),
        count,
        history_prior: args.history_prior,
        prior_violated: outcome.prior_violated,
        submitted,
        submit_error,
    })
}

#[derive(Default)]
struct Summary {
    total: usize,
    max: usize,
    games: usize,
    violations: usize,
}

impl Summary {
    fn add(&mut self, outcome: &solver::Outcome) {
        self.total += outcome.guesses.len();
        self.max = self.max.max(outcome.guesses.len());
        self.games += 1;
        self.violations += usize::from(outcome.prior_violated);
    }

    fn line(&self) -> String {
        format!(
            "mean {:.4}, max {}, prior violations {}",
            self.total as f64 / self.games.max(1) as f64,
            self.max,
            self.violations
        )
    }
}

pub fn run_bench(
    db: &[Character],
    supabase: Option<&Supabase>,
    today: NaiveDate,
    args: &BenchArgs,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    let table = Table::new(db);
    // Memo is keyed by (candidates, blocked names), independent of the prior: one solver per probe mode.
    let mut solver = Solver::new(&table, db, Probes::Free);
    let all = all(db);
    let start = Instant::now();
    let total = solver
        .total_cost(&all)
        .context("no winning strategy for this DB")?;
    let opener = solver
        .best_guess(&all)
        .context("no winning strategy for this DB")? as usize;
    let mut histogram: BTreeMap<usize, usize> = BTreeMap::new();
    for t in 0..db.len() {
        let outcome = solver::play(&mut solver, all.clone(), &mut Oracle::new(&table, t))?;
        *histogram.entry(outcome.guesses.len()).or_default() += 1;
    }
    let stats = solver.stats();
    writeln!(out, "model (uniform belief over {} characters)", db.len())?;
    writeln!(
        out,
        "  expected guesses: {:.6} ({total} / {})",
        total as f64 / db.len() as f64,
        db.len()
    )?;
    writeln!(
        out,
        "  opener: {} ({})",
        db[opener].name, db[opener].anime.name
    )?;
    for (guesses, targets) in &histogram {
        writeln!(out, "  {guesses} guesses: {targets} targets")?;
    }
    writeln!(
        out,
        "  states: {}, guesses evaluated: {}",
        stats.states, stats.guesses_evaluated
    )?;
    writeln!(out, "  wall time: {:.2?}", start.elapsed())?;

    if !args.history_prior {
        return Ok(());
    }
    let supabase = supabase.context("--history-prior needs SUPABASE_URL and SUPABASE_ANON_KEY")?;
    let rows = supabase.past_picks(today)?;
    let (picks, _) = history::resolve(db, &rows);
    let skipped = rows
        .iter()
        .filter(|(day, name)| *day >= args.since && !db.iter().any(|c| &c.name == name))
        .count();
    let mut safe = Solver::new(&table, db, Probes::Safe);
    let (mut uniform, mut prior) = (Summary::default(), Summary::default());
    for pick in picks.iter().filter(|p| p.day >= args.since) {
        uniform.add(&solver::play(
            &mut solver,
            all.clone(),
            &mut Oracle::new(&table, pick.character),
        )?);
        // `support` only looks at picks before `pick.day`.
        let support = history::support(db, &picks, pick.day, &args.rule.rule());
        prior.add(&solver::play(
            &mut safe,
            support,
            &mut Oracle::new(&table, pick.character),
        )?);
    }
    writeln!(
        out,
        "replay of past dailies since {} (today's DB, not a reconstruction of each day's game)",
        args.since
    )?;
    writeln!(out, "  days evaluated: {}", uniform.games)?;
    writeln!(out, "  days skipped (target not in DB): {skipped}")?;
    writeln!(out, "  uniform: {}", uniform.line())?;
    writeln!(out, "  history prior: {}", prior.line())?;
    Ok(())
}

pub fn run_history_check(
    db: &[Character],
    supabase: &Supabase,
    today: NaiveDate,
    args: &HistoryCheckArgs,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    let (picks, unknown) = past_picks(db, supabase, today)?;
    let gaps = history::min_gaps(db, &picks, args.since);
    let verdict = |gap: Option<i64>, window: i64| match gap {
        Some(g) if g < window => format!("CONTRADICTS the {window}-day window"),
        _ => format!("consistent with the {window}-day window"),
    };
    writeln!(
        out,
        "picks since {}: {} ({} past picks not in the DB)",
        args.since, gaps.picks, unknown
    )?;
    writeln!(
        out,
        "  min days between repeats of a character: {:?} → {}",
        gaps.min_character_gap,
        verdict(gaps.min_character_gap, args.rule.character_days)
    )?;
    writeln!(
        out,
        "  min days between repeats of an anime: {:?} → {}",
        gaps.min_anime_gap,
        verdict(gaps.min_anime_gap, args.rule.anime_days)
    )?;
    writeln!(
        out,
        "  (gaps can falsify the rule, not prove the picker enforces it)"
    )?;
    Ok(())
}

pub fn run_certify(db: &[Character], out: &mut impl Write) -> anyhow::Result<()> {
    let table = Table::new(db);
    let all = all(db);
    let start = Instant::now();
    let (total, cert) = {
        let mut solver = Solver::new(&table, db, Probes::Free);
        let total = solver
            .total_cost(&all)
            .context("no winning strategy for this DB")?;
        // The solver (and its memo) is dropped here; only the exported certificate is kept.
        (total, solver.certificate())
    };
    let search_time = start.elapsed();
    let start = Instant::now();
    let stats = certify::verify(&table, db, true, &cert, &all, total)?;
    writeln!(
        out,
        "proved optimal: {total} total guesses over {} characters = {:.6} average",
        db.len(),
        total as f64 / db.len() as f64
    )?;
    writeln!(out, "  certificate entries: {}", cert.entries.len())?;
    writeln!(
        out,
        "  checked: {} lower-bound states, {} upper-bound states, {} infeasible states, {} partitions",
        stats.lower_states, stats.upper_states, stats.infeasible_states, stats.partitions
    )?;
    writeln!(
        out,
        "  search time: {search_time:.2?}, check time: {:.2?}",
        start.elapsed()
    )?;
    Ok(())
}
