use anime_wordle_solver::cli::{self, Cli, Cmd};
use anime_wordle_solver::{data, supabase::Supabase};
use anyhow::Context;
use clap::Parser;
use std::io::Write;
use std::process::ExitCode;

/// The same public values the deployed site embeds as VITE_SUPABASE_URL / VITE_SUPABASE_ANON_KEY.
fn supabase() -> anyhow::Result<Supabase> {
    let url = std::env::var("SUPABASE_URL").context("SUPABASE_URL is not set")?;
    let key = std::env::var("SUPABASE_ANON_KEY").context("SUPABASE_ANON_KEY is not set")?;
    Ok(Supabase::new(&url, &key))
}

fn main() -> anyhow::Result<ExitCode> {
    let cli = Cli::parse();
    let today = chrono::Utc::now().date_naive();
    let db = data::load(&cli.characters)?;
    let mut out = std::io::stdout().lock();
    match cli.cmd {
        Cmd::Play(args) => {
            let report = cli::run_play(&db, &supabase()?, today, &args)?;
            writeln!(out, "{}", serde_json::to_string(&report)?)?;
            if report.submit_error.is_some() {
                return Ok(ExitCode::FAILURE);
            }
        }
        Cmd::Bench(args) => {
            let supabase = if args.history_prior {
                Some(supabase()?)
            } else {
                None
            };
            cli::run_bench(&db, supabase.as_ref(), today, &args, &mut out)?;
        }
        Cmd::HistoryCheck(args) => {
            cli::run_history_check(&db, &supabase()?, today, &args, &mut out)?
        }
    }
    Ok(ExitCode::SUCCESS)
}
