use anime_wordle_solver::cli::{Cli, Cmd, run_bench, run_history_check, run_play};
use anime_wordle_solver::data::{self, Character};
use anime_wordle_solver::supabase::Supabase;
use chrono::NaiveDate;
use clap::Parser;
use mockito::{Matcher, Server, ServerGuard};

const TODAY: &str = "2026-10-07";

fn today() -> NaiveDate {
    TODAY.parse().unwrap()
}

/// First 60 repo characters: small enough that every solve is instant. Index 0 is Monkey D. Luffy.
fn small_db() -> Vec<Character> {
    let json = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../src/data/characters.json"
    ))
    .unwrap();
    let mut db = data::parse(&json).unwrap();
    db.truncate(60);
    db
}

fn server_with_today(name: &str) -> ServerGuard {
    let mut server = Server::new();
    server
        .mock("GET", "/rest/v1/daily_index")
        .match_query(Matcher::UrlEncoded("day".into(), format!("eq.{TODAY}")))
        .with_body(format!(
            r#"[{{"day":"{TODAY}","character_name":"{name}"}}]"#
        ))
        .create();
    server
}

fn play_args(extra: &[&str]) -> anime_wordle_solver::cli::PlayArgs {
    let argv = [&["solver", "play"], extra].concat();
    match Cli::try_parse_from(argv).unwrap().cmd {
        Cmd::Play(args) => args,
        _ => unreachable!(),
    }
}

#[test]
fn submit_requires_a_bot_username() {
    assert!(Cli::try_parse_from(["solver", "play", "--submit"]).is_err());
    assert!(Cli::try_parse_from(["solver", "play", "--submit", "--username", "alice"]).is_err());
    assert!(
        Cli::try_parse_from(["solver", "play", "--submit", "--username", "solver-bot"]).is_ok()
    );
    assert!(Cli::try_parse_from(["solver", "play", "--submit", "--username", "SolverBot"]).is_ok());
}

#[test]
fn play_wins_today_and_does_not_submit_by_default() {
    let db = small_db();
    let mut server = server_with_today("Monkey D. Luffy");
    let post = server
        .mock("POST", "/rest/v1/daily_scores")
        .expect(0)
        .create();
    let report = run_play(
        &db,
        &Supabase::new(&server.url(), "k"),
        today(),
        &play_args(&[]),
    )
    .unwrap();
    assert_eq!(report.guesses.last().unwrap().name, "Monkey D. Luffy");
    assert_eq!(report.count, report.guesses.len());
    assert!(!report.submitted && report.submit_error.is_none() && !report.prior_violated);
    post.assert();
}

#[test]
fn play_submits_once_with_the_guess_count() {
    let db = small_db();
    let mut server = server_with_today("Monkey D. Luffy");
    let args = play_args(&["--submit", "--username", "solver-bot"]);
    // Guess count is deterministic: learn it from a dry run, then expect exactly that in the POST body.
    let dry = run_play(
        &db,
        &Supabase::new(&server.url(), "k"),
        today(),
        &play_args(&[]),
    )
    .unwrap();
    let post = server
        .mock("POST", "/rest/v1/daily_scores")
        .match_body(Matcher::Json(serde_json::json!({
            "day": TODAY, "username": "solver-bot", "guesses": dry.count, "hint_used": false
        })))
        .with_status(201)
        .expect(1)
        .create();
    let report = run_play(&db, &Supabase::new(&server.url(), "k"), today(), &args).unwrap();
    assert!(report.submitted && report.submit_error.is_none());
    post.assert();
}

#[test]
fn failed_submission_still_reports_the_game() {
    let db = small_db();
    let mut server = server_with_today("Monkey D. Luffy");
    server
        .mock("POST", "/rest/v1/daily_scores")
        .with_status(500)
        .create();
    let report = run_play(
        &db,
        &Supabase::new(&server.url(), "k"),
        today(),
        &play_args(&["--submit", "--username", "bot"]),
    )
    .unwrap();
    assert!(!report.submitted && report.submit_error.is_some());
    assert_eq!(report.guesses.last().unwrap().name, "Monkey D. Luffy");
}

#[test]
fn play_with_history_prior_uses_past_rows() {
    let db = small_db();
    let mut server = server_with_today("Monkey D. Luffy");
    // Luffy himself picked 10 days ago: the (wrong here) prior excludes him, the bot must still win and say so.
    server
        .mock("GET", "/rest/v1/daily_index")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("day".into(), format!("lt.{TODAY}")),
            Matcher::UrlEncoded("offset".into(), "0".into()),
        ]))
        .with_body(r#"[{"day":"2026-09-27","character_name":"Monkey D. Luffy"}]"#)
        .create();
    server
        .mock("GET", "/rest/v1/daily_index")
        .match_query(Matcher::UrlEncoded("offset".into(), "1".into()))
        .with_body("[]")
        .create();
    let report = run_play(
        &db,
        &Supabase::new(&server.url(), "k"),
        today(),
        &play_args(&["--history-prior"]),
    )
    .unwrap();
    assert_eq!(report.guesses.last().unwrap().name, "Monkey D. Luffy");
    assert!(report.history_prior && report.prior_violated);
}

fn bench_args(extra: &[&str]) -> anime_wordle_solver::cli::BenchArgs {
    let argv = [&["solver", "bench"], extra].concat();
    match Cli::try_parse_from(argv).unwrap().cmd {
        Cmd::Bench(args) => args,
        _ => unreachable!(),
    }
}

#[test]
fn bench_without_prior_is_offline() {
    let mut out = Vec::new();
    run_bench(&small_db(), None, today(), &bench_args(&[]), &mut out).unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains("expected guesses:"), "{out}");
    assert!(out.contains("opener:"), "{out}");
}

#[test]
fn bench_with_prior_needs_supabase() {
    let mut out = Vec::new();
    assert!(
        run_bench(
            &small_db(),
            None,
            today(),
            &bench_args(&["--history-prior"]),
            &mut out
        )
        .is_err()
    );
}

#[test]
fn bench_with_prior_replays_past_days_in_both_modes() {
    let db = small_db();
    let names: Vec<&str> = db.iter().skip(1).take(3).map(|c| c.name.as_str()).collect();
    let mut server = Server::new();
    let rows = format!(
        r#"[{{"day":"2026-09-01","character_name":"{}"}},{{"day":"2026-09-02","character_name":"{}"}},{{"day":"2026-09-03","character_name":"{}"}},{{"day":"2026-09-04","character_name":"Not In DB"}}]"#,
        names[0], names[1], names[2]
    );
    server
        .mock("GET", "/rest/v1/daily_index")
        .match_query(Matcher::UrlEncoded("offset".into(), "0".into()))
        .with_body(rows)
        .create();
    server
        .mock("GET", "/rest/v1/daily_index")
        .match_query(Matcher::UrlEncoded("offset".into(), "4".into()))
        .with_body("[]")
        .create();
    let mut out = Vec::new();
    run_bench(
        &db,
        Some(&Supabase::new(&server.url(), "k")),
        today(),
        &bench_args(&["--history-prior"]),
        &mut out,
    )
    .unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains("days evaluated: 3"), "{out}");
    assert!(out.contains("days skipped (target not in DB): 1"), "{out}");
    assert!(out.contains("uniform:"), "{out}");
    assert!(out.contains("history prior:"), "{out}");
}

#[test]
fn history_check_flags_a_repeat_inside_the_window() {
    let db = small_db();
    let mut server = Server::new();
    let (a, b) = (&db[1].name, &db[2].name);
    let rows = format!(
        r#"[{{"day":"2026-08-01","character_name":"{a}"}},{{"day":"2026-08-11","character_name":"{a}"}},{{"day":"2026-09-30","character_name":"{b}"}}]"#
    );
    server
        .mock("GET", "/rest/v1/daily_index")
        .match_query(Matcher::UrlEncoded("offset".into(), "0".into()))
        .with_body(rows)
        .create();
    server
        .mock("GET", "/rest/v1/daily_index")
        .match_query(Matcher::UrlEncoded("offset".into(), "3".into()))
        .with_body("[]")
        .create();
    let args = match Cli::try_parse_from(["solver", "history-check"])
        .unwrap()
        .cmd
    {
        Cmd::HistoryCheck(args) => args,
        _ => unreachable!(),
    };
    let mut out = Vec::new();
    run_history_check(
        &db,
        &Supabase::new(&server.url(), "k"),
        today(),
        &args,
        &mut out,
    )
    .unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains("picks since 2026-07-14: 3"), "{out}");
    assert!(
        out.contains("character: Some(10) → CONTRADICTS the 90-day window"),
        "{out}"
    );
    assert!(
        out.contains("anime: Some(10) → CONTRADICTS the 30-day window"),
        "{out}"
    );
}
