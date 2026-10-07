use crate::data::Character;
use crate::feedback::{Feedback, Table, WIN};
use crate::oracle::Game;
use std::collections::HashMap;

pub type Set = Vec<u16>;
type Bits = [u64; 8];

fn bits(items: impl IntoIterator<Item = u16>) -> Bits {
    let mut b = [0u64; 8];
    for t in items {
        b[t as usize / 64] |= 1 << (t % 64);
    }
    b
}

fn has(b: &Bits, t: u16) -> bool {
    b[t as usize / 64] & (1 << (t % 64)) != 0
}

pub struct Stats {
    pub states: usize,
    pub guesses_evaluated: u64,
}

pub struct Solver<'a> {
    table: &'a Table,
    /// Index of the first character with the same name: the name's id.
    name_id: Vec<u16>,
    /// Whether another character in the DB shares this character's name.
    has_namesake: Vec<bool>,
    memo: HashMap<(Bits, Bits), Option<(u32, u16)>>,
    guesses_evaluated: u64,
}

impl<'a> Solver<'a> {
    pub fn new(table: &'a Table, db: &[Character]) -> Self {
        assert_eq!(table.len(), db.len());
        let name_id: Vec<u16> = db
            .iter()
            .map(|c| db.iter().position(|o| o.name == c.name).unwrap() as u16)
            .collect();
        let has_namesake = name_id
            .iter()
            .map(|id| name_id.iter().filter(|o| *o == id).count() > 1)
            .collect();
        Self {
            table,
            name_id,
            has_namesake,
            memo: HashMap::new(),
            guesses_evaluated: 0,
        }
    }

    pub fn stats(&self) -> Stats {
        Stats {
            states: self.memo.len(),
            guesses_evaluated: self.guesses_evaluated,
        }
    }

    /// UI rule: the name wasn't guessed before. Strategy rules (a guessed name blocks its namesakes for good):
    /// a candidate can't be guessed while a namesake is still a candidate, and a non-candidate probe can't have namesakes.
    fn allowed(&self, g: usize, set: &[u16], in_set: &Bits, blocked: &Bits) -> bool {
        let id = self.name_id[g];
        if has(blocked, id) {
            return false;
        }
        if has(in_set, g as u16) {
            !set.iter()
                .any(|&t| t as usize != g && self.name_id[t as usize] == id)
        } else {
            !self.has_namesake[g]
        }
    }

    /// Adds every character sharing a name with a member, so a wrong prior can never cause a name lockout.
    fn close_namesakes(&self, set: Set) -> Set {
        let ids = bits(set.iter().map(|&t| self.name_id[t as usize]));
        (0..self.table.len() as u16)
            .filter(|&t| has(&ids, self.name_id[t as usize]))
            .collect()
    }

    fn check(set: &[u16]) {
        assert!(
            !set.is_empty() && set.windows(2).all(|w| w[0] < w[1]),
            "candidate set must be non-empty, sorted, unique"
        );
    }

    /// Non-winning parts of `set` under `g`, largest first then by first member; None if `g` gives no information.
    fn partition(&self, g: usize, set: &[u16]) -> Option<Vec<Set>> {
        let mut pairs: Vec<(Feedback, u16)> = set
            .iter()
            .map(|&t| (self.table.get(g, t as usize), t))
            .collect();
        pairs.sort_unstable();
        let mut parts: Vec<Set> = Vec::new();
        let mut won = false;
        for chunk in pairs.chunk_by(|a, b| a.0 == b.0) {
            if chunk[0].0 == WIN {
                won = true;
            } else {
                parts.push(chunk.iter().map(|&(_, t)| t).collect());
            }
        }
        if !won && parts.len() == 1 {
            return None;
        }
        parts.sort_by(|a, b| b.len().cmp(&a.len()).then(a[0].cmp(&b[0])));
        Some(parts)
    }

    /// (summed guesses over all targets, best guess), or None if no allowed strategy always wins.
    fn solve(&mut self, set: &[u16], blocked: Bits) -> Option<(u32, u16)> {
        if set.len() == 1 {
            return (!has(&blocked, self.name_id[set[0] as usize])).then_some((1, set[0]));
        }
        let key = (bits(set.iter().copied()), blocked);
        if let Some(&hit) = self.memo.get(&key) {
            return hit;
        }
        let in_set = key.0;
        let mut candidates: Vec<(usize, bool, usize, Vec<Set>)> = (0..self.table.len())
            .filter(|&g| self.allowed(g, set, &in_set, &blocked))
            .filter_map(|g| {
                let parts = self.partition(g, set)?;
                let spread = parts.iter().map(|p| p.len() * p.len()).sum::<usize>();
                Some((spread, !has(&in_set, g as u16), g, parts))
            })
            .collect();
        candidates.sort_unstable_by_key(|c| (c.0, c.1, c.2));
        let mut best: Option<(u32, u16)> = None;
        for (_, _, g, parts) in candidates {
            self.guesses_evaluated += 1;
            let mut child_blocked = blocked;
            let id = self.name_id[g];
            child_blocked[id as usize / 64] |= 1 << (id % 64);
            let bound = |p: &Set| 2 * p.len() as u32 - 1;
            let mut cost = set.len() as u32 + parts.iter().map(bound).sum::<u32>();
            let mut feasible = true;
            for p in &parts {
                if best.is_some_and(|(b, _)| cost >= b) {
                    break;
                }
                match self.solve(p, child_blocked) {
                    Some((c, _)) => cost += c - bound(p),
                    None => {
                        feasible = false;
                        break;
                    }
                }
            }
            if feasible && best.is_none_or(|(b, _)| cost < b) {
                best = Some((cost, g as u16));
            }
        }
        self.memo.insert(key, best);
        best
    }

    pub fn total_cost(&mut self, set: &[u16]) -> Option<u32> {
        Self::check(set);
        self.solve(set, [0; 8]).map(|(c, _)| c)
    }

    pub fn expected_cost(&mut self, set: &[u16]) -> Option<f64> {
        self.total_cost(set).map(|c| c as f64 / set.len() as f64)
    }

    pub fn best_guess(&mut self, set: &[u16]) -> Option<u16> {
        Self::check(set);
        self.solve(set, [0; 8]).map(|(_, g)| g)
    }
}

pub struct Outcome {
    pub guesses: Vec<usize>,
    pub prior_violated: bool,
}

fn consistent(table: &Table, history: &[(usize, Feedback)]) -> Set {
    (0..table.len() as u16)
        .filter(|&t| history.iter().all(|&(g, f)| table.get(g, t as usize) == f))
        .collect()
}

pub fn play(solver: &mut Solver, support: Set, game: &mut impl Game) -> anyhow::Result<Outcome> {
    let table = solver.table;
    let initial = bits(support.iter().copied());
    let mut history: Vec<(usize, Feedback)> = Vec::new();
    let mut prior_violated = support.is_empty();
    let mut set = if prior_violated {
        consistent(table, &history)
    } else {
        Solver::check(&support);
        solver.close_namesakes(support)
    };
    let mut blocked = [0u64; 8];
    loop {
        let (_, g) = solver
            .solve(&set, blocked)
            .ok_or_else(|| anyhow::anyhow!("no allowed strategy wins from this state"))?;
        let g = g as usize;
        let fb = game.guess(g);
        history.push((g, fb));
        let id = solver.name_id[g];
        blocked[id as usize / 64] |= 1 << (id % 64);
        if fb == WIN {
            prior_violated |= !has(&initial, g as u16);
            return Ok(Outcome {
                guesses: history.into_iter().map(|(g, _)| g).collect(),
                prior_violated,
            });
        }
        set.retain(|&t| table.get(g, t as usize) == fb);
        if set.is_empty() {
            anyhow::ensure!(
                !prior_violated,
                "no character is consistent with the feedback"
            );
            prior_violated = true;
            set = consistent(table, &history);
            anyhow::ensure!(
                !set.is_empty(),
                "no character is consistent with the feedback"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{Character, parse};
    use crate::oracle::Oracle;

    fn repo_db() -> Vec<Character> {
        parse(
            &std::fs::read_to_string(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../src/data/characters.json"
            ))
            .unwrap(),
        )
        .unwrap()
    }

    fn lcg(s: &mut u64) -> usize {
        *s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (*s >> 33) as usize
    }

    /// k distinct indices out of 0..n, fixed seed.
    fn sample(n: usize, k: usize, seed: u64) -> Vec<usize> {
        let mut s = seed;
        let mut idx: Vec<usize> = (0..n).collect();
        for i in 0..k {
            let j = i + lcg(&mut s) % (n - i);
            idx.swap(i, j);
        }
        idx.truncate(k);
        idx
    }

    /// Same recurrence and action rules, no pruning, no memo.
    fn brute(db: &[Character], table: &Table, set: &[u16], blocked: &[String]) -> Option<u32> {
        if set.len() == 1 && !blocked.contains(&db[set[0] as usize].name) {
            return Some(1);
        }
        (0..table.len())
            .filter(|&g| {
                let namesake = |t: usize| t != g && db[t].name == db[g].name;
                !blocked.contains(&db[g].name)
                    && if set.contains(&(g as u16)) {
                        !set.iter().any(|&t| namesake(t as usize))
                    } else {
                        !(0..db.len()).any(namesake)
                    }
            })
            .filter_map(|g| {
                let mut parts: std::collections::BTreeMap<Feedback, Vec<u16>> = Default::default();
                for &t in set {
                    parts.entry(table.get(g, t as usize)).or_default().push(t);
                }
                if parts.len() == 1 && !parts.contains_key(&WIN) {
                    return None;
                }
                let mut b = blocked.to_vec();
                b.push(db[g].name.clone());
                let mut sum = set.len() as u32;
                for (f, p) in &parts {
                    if *f != WIN {
                        sum += brute(db, table, p, &b)?;
                    }
                }
                Some(sum)
            })
            .min()
    }

    #[test]
    fn matches_brute_force_on_subsets_of_a_larger_guess_pool() {
        let db = repo_db();
        for seed in 0..30 {
            let pool: Vec<Character> = sample(db.len(), 20, seed)
                .into_iter()
                .map(|i| db[i].clone())
                .collect();
            let table = Table::new(&pool);
            let set: Vec<u16> = {
                let mut s: Vec<u16> = sample(20, 6, seed + 100)
                    .into_iter()
                    .map(|i| i as u16)
                    .collect();
                s.sort();
                s
            };
            assert_eq!(
                Solver::new(&table, &pool).total_cost(&set),
                brute(&pool, &table, &set, &[]),
                "seed {seed}"
            );
        }
    }

    /// Found by scanning ~3600 random configurations: on these, greedily taking the best-spread guess
    /// costs one extra guess in total, so they fail if pruning ever degrades the search to greedy.
    #[test]
    fn matches_brute_force_where_greedy_is_suboptimal() {
        let db = repo_db();
        for (pool_size, seed, optimum) in
            [(16, 77, 21), (16, 178, 22), (16, 386, 21), (24, 119, 21)]
        {
            let pool: Vec<Character> = sample(db.len(), pool_size, seed)
                .into_iter()
                .map(|i| db[i].clone())
                .collect();
            let table = Table::new(&pool);
            let mut set: Vec<u16> = sample(pool_size, 10, seed + 1000)
                .into_iter()
                .map(|i| i as u16)
                .collect();
            set.sort();
            assert_eq!(
                brute(&pool, &table, &set, &[]),
                Some(optimum),
                "fixture drifted: pool={pool_size} seed={seed}"
            );
            assert_eq!(
                Solver::new(&table, &pool).total_cost(&set),
                Some(optimum),
                "pool={pool_size} seed={seed}"
            );
        }
    }

    /// Candidates from one anime share year and studio, so parts stay large and pruning is actually exercised.
    #[test]
    fn matches_brute_force_on_same_anime_candidate_sets() {
        let db = repo_db();
        for anime in [
            "Love Live!",
            "Digimon",
            "Clannad",
            "Fire Force",
            "Love Hina",
            "Sakura Taisen",
        ] {
            let mut idx: Vec<usize> = (0..db.len())
                .filter(|&i| db[i].anime.name == anime)
                .collect();
            let n = idx.len();
            idx.extend(
                sample(db.len(), 20, n as u64)
                    .into_iter()
                    .filter(|&i| db[i].anime.name != anime)
                    .take(12),
            );
            let pool: Vec<Character> = idx.iter().map(|&i| db[i].clone()).collect();
            let table = Table::new(&pool);
            let set: Vec<u16> = (0..n as u16).collect();
            assert_eq!(
                Solver::new(&table, &pool).total_cost(&set),
                brute(&pool, &table, &set, &[]),
                "{anime}"
            );
        }
    }

    #[test]
    fn duplicate_names_follow_ui_rules_in_brute_force_comparison() {
        // Pool containing both Kaguras plus some others, with both Kaguras as candidates.
        let db = repo_db();
        let k: Vec<usize> = db
            .iter()
            .enumerate()
            .filter(|(_, c)| c.name == "Kagura")
            .map(|(i, _)| i)
            .collect();
        let mut idx = sample(db.len(), 14, 7);
        idx.retain(|i| !k.contains(i));
        idx.extend(&k);
        let pool: Vec<Character> = idx.iter().map(|&i| db[i].clone()).collect();
        let table = Table::new(&pool);
        let n = pool.len() as u16;
        let set: Vec<u16> = vec![0, 1, 2, n - 2, n - 1];
        assert_eq!(
            Solver::new(&table, &pool).total_cost(&set),
            brute(&pool, &table, &set, &[])
        );
    }

    /// 40 sampled characters plus both Kaguras (last two indices). Small enough to solve instantly.
    fn kagura_pool() -> Vec<Character> {
        let db = repo_db();
        let k: Vec<usize> = db
            .iter()
            .enumerate()
            .filter(|(_, c)| c.name == "Kagura")
            .map(|(i, _)| i)
            .collect();
        let mut idx = sample(db.len(), 42, 11);
        idx.retain(|i| !k.contains(i));
        idx.truncate(40);
        idx.extend(&k);
        idx.iter().map(|&i| db[i].clone()).collect()
    }

    #[test]
    fn never_guesses_a_name_twice_and_wins_both_kaguras() {
        let pool = kagura_pool();
        let table = Table::new(&pool);
        let mut solver = Solver::new(&table, &pool);
        let all: Set = (0..pool.len() as u16).collect();
        for t in [pool.len() - 2, pool.len() - 1] {
            let out = play(&mut solver, all.clone(), &mut Oracle::new(&table, t)).unwrap();
            assert_eq!(*out.guesses.last().unwrap(), t);
            let names: Vec<&str> = out.guesses.iter().map(|&g| pool[g].name.as_str()).collect();
            let unique: std::collections::HashSet<_> = names.iter().collect();
            assert_eq!(unique.len(), names.len(), "{names:?}");
        }
    }

    #[test]
    fn only_the_two_kaguras_is_unwinnable_under_ui_rules() {
        let db = repo_db();
        let pool: Vec<Character> = db.iter().filter(|c| c.name == "Kagura").cloned().collect();
        let table = Table::new(&pool);
        assert_eq!(Solver::new(&table, &pool).total_cost(&[0, 1]), None);
    }

    #[test]
    fn prior_excluding_the_real_kagura_still_wins() {
        // Support holds only the other Kagura; the real target is excluded by the (wrong) prior.
        let pool = kagura_pool();
        let table = Table::new(&pool);
        let (real, other) = (pool.len() - 2, pool.len() - 1);
        for (support, target) in [
            (vec![other as u16], real),
            (vec![real as u16], other),
            (vec![0u16], real),
        ] {
            let out = play(
                &mut Solver::new(&table, &pool),
                support,
                &mut Oracle::new(&table, target),
            )
            .unwrap();
            assert_eq!(*out.guesses.last().unwrap(), target);
            assert!(out.prior_violated);
        }
    }

    #[test]
    fn deterministic_across_fresh_solvers() {
        let pool = kagura_pool();
        let table = Table::new(&pool);
        let all: Set = (0..pool.len() as u16).collect();
        let run = |t| {
            play(
                &mut Solver::new(&table, &pool),
                all.clone(),
                &mut Oracle::new(&table, t),
            )
            .unwrap()
            .guesses
        };
        for t in 0..pool.len() {
            assert_eq!(run(t), run(t));
        }
    }

    #[test]
    fn empty_support_recovers_and_reports_violation() {
        let pool = kagura_pool();
        let table = Table::new(&pool);
        let out = play(
            &mut Solver::new(&table, &pool),
            vec![],
            &mut Oracle::new(&table, 5),
        )
        .unwrap();
        assert_eq!(*out.guesses.last().unwrap(), 5);
        assert!(out.prior_violated);
    }

    #[test]
    fn excluded_target_still_wins_and_reports_violation() {
        let pool = kagura_pool();
        let table = Table::new(&pool);
        let mut solver = Solver::new(&table, &pool);
        for target in 0..pool.len() {
            let support: Set = (0..pool.len() as u16)
                .filter(|&t| t as usize != target)
                .collect();
            let out = play(&mut solver, support, &mut Oracle::new(&table, target)).unwrap();
            assert_eq!(*out.guesses.last().unwrap(), target);
            assert!(out.prior_violated);
        }
    }

    #[test]
    #[should_panic]
    fn empty_set_is_rejected() {
        let pool = kagura_pool();
        let table = Table::new(&pool);
        Solver::new(&table, &pool).total_cost(&[]);
    }

    #[test]
    fn full_db_kaguras_have_different_name_separators() {
        let db = repo_db();
        let table = Table::new(&db);
        let a = db.iter().position(|c| c.name == "Kagura").unwrap();
        let b = db.iter().rposition(|c| c.name == "Kagura").unwrap();
        assert!(
            (0..db.len()).any(|g| db[g].name != "Kagura" && table.get(g, a) != table.get(g, b))
        );
    }

    /// Under uniform belief, the summed play lengths over all targets equal `total_cost` exactly.
    #[test]
    fn full_db_every_target_wins_and_mean_equals_expected_cost() {
        let db = repo_db();
        let table = Table::new(&db);
        let mut solver = Solver::new(&table, &db);
        let all: Set = (0..db.len() as u16).collect();
        let total = solver.total_cost(&all).unwrap();
        let mut played = 0u32;
        for t in 0..db.len() {
            let out = play(&mut solver, all.clone(), &mut Oracle::new(&table, t)).unwrap();
            assert_eq!(*out.guesses.last().unwrap(), t);
            assert!(!out.prior_violated);
            played += out.guesses.len() as u32;
        }
        assert_eq!(played, total);
    }
}
