# Anime Wordle solver

A bot that plays the [Anime Wordle](https://hewigkeyt.github.io/anime_wordle/) daily with the strategy that minimises the expected number of guesses. It only uses what a player can see. It is meant to run headless on a server as a baseline, under a username that clearly marks it as a bot.

## How it plays

- The feedback for every (guess, target) pair is a port of `buildResult` in `src/utils/gameLogic.js`. A test checks the port against the real JS for every pair in the character DB.
- An exact branch-and-bound search finds the strategy with the lowest expected number of guesses, assuming every candidate is equally likely. With the current 488 characters:
  - the expected number of guesses is 3.0246
  - the opener is Kagome Higurashi
  - the full search takes about 27 s and 250 MB
- The bot only makes guesses the UI allows. Once a name has been guessed, the suggestions hide every character with that name (this matters for the two "Kagura"). The search also never makes a guess that could leave the real target impossible to enter.

## No cheating

- The day's answer is fetched from Supabase the same way the web page does, but only inside `Oracle::daily` (`src/oracle.rs`). The oracle only answers "what feedback does this guess get". The solver never sees the target, and error messages never include it.
- The solver's belief is uniform over the character list. It does not exploit implementation details a player can't see. Example: the daily picker stores only a name, so one of the two Kaguras can never be picked. The solver ignores this.
- The optional history prior only reads past answers (`day < today`), which the page shows every day as "Yesterday's character". Any row for today or later is an error.

## Usage

```bash
cargo build --release
export SUPABASE_URL=...        # the public values embedded in the deployed site's JS bundle
export SUPABASE_ANON_KEY=...   # (VITE_SUPABASE_URL / VITE_SUPABASE_ANON_KEY at build time)

./target/release/anime-wordle-solver play                    # play today's daily, print a JSON report
./target/release/anime-wordle-solver play --history-prior    # same, excluding recently picked characters/anime
./target/release/anime-wordle-solver bench                   # offline: expected guesses, opener, histogram
./target/release/anime-wordle-solver bench --history-prior --since 2026-08-05   # replay past dailies, both modes
./target/release/anime-wordle-solver history-check --since 2026-08-05           # test the footer rule on past dailies
```

- `--characters <path|url>` changes the character DB. The default is the GitHub file the daily picker reads.
- The `play` report includes every guess, so its last guess is today's answer.

### Submitting to the leaderboard

Submission is off by default: `--submit --username <name>`, where the name must contain "bot". The bot submits before printing the report. If submission fails, the report shows `submit_error` and the exit code is non-zero.

### Cron

`play` needs the day's row to exist, so run it shortly after 00:00 UTC:

```
CRON_TZ=UTC
5 0 * * * cd /path/to/solver && ./target/release/anime-wordle-solver play --history-prior >> runs.jsonl
```

Some cron implementations ignore `CRON_TZ`. In that case, use the server's local time for 00:05 UTC (09:05 JST).

## The history prior (experimental)

The site footer says the same character can't be picked twice within 3 months and the same anime can't appear twice within a month. `--history-prior` removes those characters from the candidates, using rolling windows: picked fewer than 90 / 30 days ago. Adjust them with `--character-days` and `--anime-days`.

- The picker in `supabase/functions/daily-picker/index.ts` doesn't implement this rule, so it describes the deployed picker, unconfirmed.
- `history-check` showed anime repeats inside 30 days until 2026-08-04. The rule was announced on 2026-07-27. Since 2026-08-05 there have been none (shortest anime gap: 31 days), and no character has repeated.
- If the prior is wrong, the bot recomputes the candidates without it, still wins, and reports `prior_violated`. A character sharing a name with a candidate is never excluded, so a wrong prior can't make the target impossible to enter.

Replay of the 63 dailies from 2026-08-05 to 2026-10-06 (`bench --history-prior --since 2026-08-05`, using the current DB):

| Mode | Mean guesses | Max | Prior violations |
|---|---|---|---|
| Uniform | 3.032 | 4 | 0 |
| History prior | 2.810 | 4 | 0 |

## Development

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

- The feedback parity test runs `node` against `../src/utils/gameLogic.js`.
- Tests are compiled with optimisations (`[profile.test]`) because the full-DB solver tests take about 30 s.
- `cargo run --release --example feasibility` times the full search on its own.
