//! Independent check that a solver's results are optimal.
//!
//! The solver exports its memo as a [`Certificate`]: for each state (candidates S, blocked name ids B) it solved,
//! either the minimum summed guess count and the guess achieving it, or "infeasible" (no strategy always wins).
//! [`Checker`] treats those values as unproven hints. It re-derives allowed guesses and feedback partitions with
//! its own code (sharing only the feedback table and the DB with the search) and proves:
//! - `lower(S, B, v)`: no allowed strategy totals fewer than v guesses;
//! - `upper(S, B, v)`: the certificate's recorded guesses total exactly v;
//! - `infeasible(S, B)`: no strategy wins on every target.
//!
//! Model (see solver/README.md): a guess is allowed if its name wasn't guessed before, no other candidate shares
//! its name, and (unless `free_probes`) a non-candidate guess has no namesake in the DB. Guesses that leave S as a
//! single non-winning part are skipped: they cost |S| and only block more names, so they are never optimal.
//! Every other part is strictly smaller than S, so the recursion is well-founded.

use crate::data::Character;
use crate::feedback::{Feedback, Table, WIN};
use anyhow::ensure;
use std::collections::{BTreeMap, HashMap};

/// (sorted candidates, sorted blocked name ids). A name id is the index of the first DB character with that name.
pub type State = (Vec<u16>, Vec<u16>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    Finite { cost: u32, guess: u16 },
    Infeasible,
}

#[derive(Default)]
pub struct Certificate {
    pub entries: HashMap<State, Entry>,
}

#[derive(Clone, Copy, Debug)]
pub struct CheckStats {
    pub lower_states: usize,
    pub upper_states: usize,
    pub infeasible_states: usize,
    pub partitions: u64,
}

const INF: u64 = 1 << 40;

pub struct Checker<'a> {
    table: &'a Table,
    cert: &'a Certificate,
    free_probes: bool,
    name_id: Vec<u16>,
    has_namesake: Vec<bool>,
    lower_proven: HashMap<State, u64>,
    upper_proven: HashMap<State, u64>,
    infeasible_proven: HashMap<State, ()>,
    partitions: u64,
}

fn blocked(b: &[u16], id: u16) -> bool {
    b.binary_search(&id).is_ok()
}

fn with_name(b: &[u16], id: u16) -> Vec<u16> {
    let mut out = b.to_vec();
    if let Err(i) = out.binary_search(&id) {
        out.insert(i, id);
    }
    out
}

impl<'a> Checker<'a> {
    pub fn new(
        table: &'a Table,
        db: &[Character],
        free_probes: bool,
        cert: &'a Certificate,
    ) -> anyhow::Result<Self> {
        let n = db.len();
        ensure!(
            table.len() == n,
            "feedback table is {}x{} but the DB has {n} characters",
            table.len(),
            table.len()
        );
        for g in 0..n {
            for t in 0..n {
                ensure!(
                    (table.get(g, t) == WIN) == (g == t),
                    "all-correct feedback must mean the guess is the target (guess {g}, target {t})"
                );
            }
        }
        for ((s, b), entry) in &cert.entries {
            let guess_ok = match entry {
                Entry::Finite { guess, .. } => (*guess as usize) < n,
                Entry::Infeasible => true,
            };
            ensure!(
                guess_ok && s.iter().chain(b).all(|&i| (i as usize) < n),
                "certificate index out of range"
            );
        }
        let name_id: Vec<u16> = db
            .iter()
            .map(|c| db.iter().position(|o| o.name == c.name).unwrap() as u16)
            .collect();
        let has_namesake = (0..n)
            .map(|i| (0..n).any(|j| j != i && name_id[j] == name_id[i]))
            .collect();
        Ok(Self {
            table,
            cert,
            free_probes,
            name_id,
            has_namesake,
            lower_proven: HashMap::new(),
            upper_proven: HashMap::new(),
            infeasible_proven: HashMap::new(),
            partitions: 0,
        })
    }

    pub fn stats(&self) -> CheckStats {
        CheckStats {
            lower_states: self.lower_proven.len(),
            upper_states: self.upper_proven.len(),
            infeasible_states: self.infeasible_proven.len(),
            partitions: self.partitions,
        }
    }

    fn allowed(&self, g: usize, s: &[u16], b: &[u16]) -> bool {
        let id = self.name_id[g];
        !blocked(b, id)
            && !s
                .iter()
                .any(|&t| t as usize != g && self.name_id[t as usize] == id)
            && (self.free_probes || s.binary_search(&(g as u16)).is_ok() || !self.has_namesake[g])
    }

    /// Non-winning parts, largest first then by first member; None if `g` gives no information.
    fn partition(&mut self, g: usize, s: &[u16]) -> Option<Vec<Vec<u16>>> {
        self.partitions += 1;
        let mut groups: BTreeMap<Feedback, Vec<u16>> = BTreeMap::new();
        for &t in s {
            groups
                .entry(self.table.get(g, t as usize))
                .or_default()
                .push(t);
        }
        let won = groups.remove(&WIN).is_some();
        if !won && groups.len() == 1 {
            return None;
        }
        let mut parts: Vec<Vec<u16>> = groups.into_values().collect();
        parts.sort_by(|a, b| b.len().cmp(&a.len()).then(a[0].cmp(&b[0])));
        Some(parts)
    }

    /// Lower bound needing no proof: a blocked singleton can't be won; otherwise one guess wins at most one target.
    fn bound(&self, p: &[u16], b: &[u16]) -> u64 {
        match p {
            [t] if blocked(b, self.name_id[*t as usize]) => INF,
            [_] => 1,
            _ => 2 * p.len() as u64 - 1,
        }
    }

    fn entry(&self, s: &[u16], b: &[u16]) -> Option<Entry> {
        self.cert.entries.get(&(s.to_vec(), b.to_vec())).copied()
    }

    /// Proves `C(S, B) >= v`.
    pub fn lower(&mut self, s: &[u16], b: &[u16], v: u64) -> bool {
        if v <= self.bound(s, b) {
            return true;
        }
        let key = (s.to_vec(), b.to_vec());
        if self.lower_proven.get(&key).is_some_and(|&p| p >= v) {
            return true;
        }
        for g in 0..self.table.len() {
            if !self.allowed(g, s, b) {
                continue;
            }
            let Some(parts) = self.partition(g, s) else {
                continue;
            };
            let b2 = with_name(b, self.name_id[g]);
            let mut bounds: Vec<u64> = parts.iter().map(|p| self.bound(p, &b2)).collect();
            let total = |bounds: &[u64]| (s.len() as u64 + bounds.iter().sum::<u64>()).min(INF);
            for (i, p) in parts.iter().enumerate() {
                if total(&bounds) >= v {
                    break;
                }
                match self.entry(p, &b2) {
                    Some(Entry::Infeasible) if self.infeasible(p, &b2) => bounds[i] = INF,
                    Some(Entry::Finite { cost, .. })
                        if cost as u64 > bounds[i] && self.lower(p, &b2, cost as u64) =>
                    {
                        bounds[i] = cost as u64
                    }
                    _ => {}
                }
            }
            if total(&bounds) < v {
                return false;
            }
        }
        let proven = self.lower_proven.entry(key).or_insert(0);
        *proven = (*proven).max(v);
        true
    }

    /// Proves the certificate's recorded guesses win on every target in S with exactly v guesses in total.
    pub fn upper(&mut self, s: &[u16], b: &[u16], v: u64) -> bool {
        if let [t] = s {
            return v == 1 && !blocked(b, self.name_id[*t as usize]);
        }
        let key = (s.to_vec(), b.to_vec());
        if self.upper_proven.get(&key) == Some(&v) {
            return true;
        }
        let Some(Entry::Finite { cost, guess }) = self.entry(s, b) else {
            return false;
        };
        let g = guess as usize;
        if cost as u64 != v || !self.allowed(g, s, b) {
            return false;
        }
        let Some(parts) = self.partition(g, s) else {
            return false;
        };
        let b2 = with_name(b, self.name_id[g]);
        let mut sum = s.len() as u64;
        for p in &parts {
            let w = match (p.as_slice(), self.entry(p, &b2)) {
                ([_], _) => 1,
                (_, Some(Entry::Finite { cost, .. })) => cost as u64,
                _ => return false,
            };
            if !self.upper(p, &b2, w) {
                return false;
            }
            sum += w;
        }
        if sum != v {
            return false;
        }
        self.upper_proven.insert(key, v);
        true
    }

    /// Proves no strategy wins on every target in S: every allowed informative guess has an infeasible part.
    pub fn infeasible(&mut self, s: &[u16], b: &[u16]) -> bool {
        if let [t] = s {
            return blocked(b, self.name_id[*t as usize]);
        }
        let key = (s.to_vec(), b.to_vec());
        if self.infeasible_proven.contains_key(&key) {
            return true;
        }
        for g in 0..self.table.len() {
            if !self.allowed(g, s, b) {
                continue;
            }
            let Some(parts) = self.partition(g, s) else {
                continue;
            };
            let b2 = with_name(b, self.name_id[g]);
            let mut stuck = false;
            for p in &parts {
                if self.bound(p, &b2) == INF
                    || (self.entry(p, &b2) == Some(Entry::Infeasible) && self.infeasible(p, &b2))
                {
                    stuck = true;
                    break;
                }
            }
            if !stuck {
                return false;
            }
        }
        self.infeasible_proven.insert(key, ());
        true
    }
}

/// Proves `claim` is the minimum summed guess count for `root` with nothing blocked, and that the certificate
/// achieves it.
pub fn verify(
    table: &Table,
    db: &[Character],
    free_probes: bool,
    cert: &Certificate,
    root: &[u16],
    claim: u32,
) -> anyhow::Result<CheckStats> {
    ensure!(
        !root.is_empty()
            && root.windows(2).all(|w| w[0] < w[1])
            && root.iter().all(|&t| (t as usize) < db.len()),
        "root must be non-empty, sorted, unique and in range"
    );
    let mut checker = Checker::new(table, db, free_probes, cert)?;
    ensure!(
        checker.lower(root, &[], claim as u64),
        "lower bound not proven: some allowed strategy may total fewer than {claim}"
    );
    ensure!(
        checker.upper(root, &[], claim as u64),
        "upper bound not proven: the certificate's strategy does not total {claim}"
    );
    Ok(checker.stats())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::parse;
    use crate::solver::{Probes, Solver};

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

    /// k distinct indices out of 0..n, fixed seed.
    fn sample(n: usize, k: usize, seed: u64) -> Vec<usize> {
        let mut s = seed;
        let mut idx: Vec<usize> = (0..n).collect();
        for i in 0..k {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let j = i + (s >> 33) as usize % (n - i);
            idx.swap(i, j);
        }
        idx.truncate(k);
        idx
    }

    /// Characters identical except for name and age, so feedback depends only on name equality and age order.
    fn synth(rows: &[(&str, i64)]) -> Vec<Character> {
        let rows: Vec<String> = rows
            .iter()
            .map(|(name, age)| format!(r#"{{"name":"{name}","anime":{{"name":"X","year":2000,"studio":"S"}},"age":{age},"height":1,"hair_color":"R","sex":null,"seiyuu":"V"}}"#))
            .collect();
        parse(&format!("[{}]", rows.join(","))).unwrap()
    }

    fn mode(free: bool) -> Probes {
        if free { Probes::Free } else { Probes::Safe }
    }

    /// Solver total and certificate for `set`.
    fn solve(table: &Table, db: &[Character], free: bool, set: &[u16]) -> (u32, Certificate) {
        let mut solver = Solver::new(table, db, mode(free));
        let total = solver.total_cost(set).unwrap();
        (total, solver.certificate())
    }

    #[test]
    fn honest_certificates_verify_on_small_pools() {
        let db = repo_db();
        let mut cases: Vec<(Vec<Character>, Vec<u16>)> = (0..10)
            .map(|seed| {
                let pool: Vec<Character> = sample(db.len(), 20, seed)
                    .into_iter()
                    .map(|i| db[i].clone())
                    .collect();
                let mut set: Vec<u16> = sample(20, 8, seed + 100)
                    .into_iter()
                    .map(|i| i as u16)
                    .collect();
                set.sort();
                (pool, set)
            })
            .collect();
        let k: Vec<usize> = (0..db.len()).filter(|&i| db[i].name == "Kagura").collect();
        let mut idx = sample(db.len(), 42, 11);
        idx.retain(|i| !k.contains(i));
        idx.truncate(40);
        idx.extend(&k);
        cases.push((
            idx.iter().map(|&i| db[i].clone()).collect(),
            (0..42).collect(),
        ));
        for (pool, set) in &cases {
            let table = Table::new(pool);
            for free in [false, true] {
                let (total, cert) = solve(&table, pool, free, set);
                verify(&table, pool, free, &cert, set, total).unwrap();
                let mut checker = Checker::new(&table, pool, free, &cert).unwrap();
                assert!(checker.lower(set, &[], total as u64));
                assert!(checker.upper(set, &[], total as u64));
            }
        }
    }

    #[test]
    fn lower_rejects_an_achieved_but_suboptimal_strategy() {
        // Guessing B (age 20) splits {A, C} into singletons: total 5. Guessing A or C first: total 6.
        let db = synth(&[("A", 10), ("B", 20), ("C", 30)]);
        let table = Table::new(&db);
        let (total, mut cert) = solve(&table, &db, true, &[0, 1, 2]);
        assert_eq!(total, 5);
        // Forge: open with A, then solve {B, C} by guessing B (blocked: A).
        cert.entries
            .insert((vec![0, 1, 2], vec![]), Entry::Finite { cost: 6, guess: 0 });
        cert.entries
            .insert((vec![1, 2], vec![0]), Entry::Finite { cost: 3, guess: 1 });
        let mut checker = Checker::new(&table, &db, true, &cert).unwrap();
        assert!(
            checker.upper(&[0, 1, 2], &[], 6),
            "the forged strategy is achievable"
        );
        assert!(
            !checker.lower(&[0, 1, 2], &[], 6),
            "but guessing B beats it"
        );
        assert!(verify(&table, &db, true, &cert, &[0, 1, 2], 6).is_err());
    }

    #[test]
    fn inflated_entries_cannot_raise_the_claim() {
        let db = synth(&[
            ("A", 1),
            ("B", 2),
            ("C", 3),
            ("D", 4),
            ("E", 5),
            ("F", 6),
            ("G", 7),
            ("H", 8),
        ]);
        let table = Table::new(&db);
        let root: Vec<u16> = (0..8).collect();
        let (total, cert) = solve(&table, &db, true, &root);
        assert!(!Checker::new(&table, &db, true, &cert).unwrap().lower(
            &root,
            &[],
            total as u64 + 1
        ));
        let mut inflated = cert;
        for entry in inflated.entries.values_mut() {
            if let Entry::Finite { cost, .. } = entry {
                *cost += 5;
            }
        }
        assert!(!Checker::new(&table, &db, true, &inflated).unwrap().lower(
            &root,
            &[],
            total as u64 + 1
        ));
    }

    #[test]
    fn infeasible_children_are_needed_and_verified() {
        // Names: A, A, B, B, X. Guessing B10 blocks name B while {A20, A30} still needs a B to split it.
        let db = synth(&[("A", 20), ("A", 30), ("B", 10), ("B", 25), ("X", 15)]);
        let table = Table::new(&db);
        let root = [0, 1, 2];
        let (total, cert) = solve(&table, &db, true, &root);
        assert_eq!(total, 8);
        verify(&table, &db, true, &cert, &root, total).unwrap();
        let mut stripped = cert;
        stripped
            .entries
            .retain(|_, e| matches!(e, Entry::Finite { .. }));
        assert!(
            !Checker::new(&table, &db, true, &stripped)
                .unwrap()
                .lower(&root, &[], 8)
        );
    }

    #[test]
    fn blocked_singleton_is_infinite() {
        let db = synth(&[("A", 1), ("B", 2)]);
        let table = Table::new(&db);
        let cert = Certificate::default();
        let mut checker = Checker::new(&table, &db, true, &cert).unwrap();
        assert!(checker.lower(&[0], &[0], 1000));
        assert!(!checker.upper(&[0], &[0], 1));
        assert!(checker.infeasible(&[0], &[0]));
        assert!(!checker.infeasible(&[0], &[1]));
        assert!(checker.upper(&[0], &[1], 1));
    }

    #[test]
    fn missing_needed_entry_fails_lower() {
        let db = synth(&[
            ("A", 1),
            ("B", 2),
            ("C", 3),
            ("D", 4),
            ("E", 5),
            ("F", 6),
            ("G", 7),
            ("H", 8),
        ]);
        let table = Table::new(&db);
        let root: Vec<u16> = (0..8).collect();
        let (total, cert) = solve(&table, &db, true, &root);
        assert_eq!(total, 21);
        verify(&table, &db, true, &cert, &root, 21).unwrap();
        // Drop the best opener's 4-character child.
        let Some(Entry::Finite { guess, .. }) = cert.entries.get(&(root.clone(), vec![])) else {
            panic!("root entry")
        };
        let child = cert
            .entries
            .keys()
            .find(|(s, b)| s.len() == 4 && b == &vec![*guess])
            .cloned()
            .expect("4-character child of the opener");
        let mut missing = cert;
        missing.entries.remove(&child);
        assert!(
            !Checker::new(&table, &db, true, &missing)
                .unwrap()
                .lower(&root, &[], 21)
        );
        let empty = Certificate::default();
        let mut checker = Checker::new(&table, &db, true, &empty).unwrap();
        assert!(!checker.lower(&root, &[], 21));
        assert!(checker.lower(&root, &[], 20));
    }

    #[test]
    fn forged_infeasible_tag_is_rejected() {
        let db = synth(&[
            ("A", 1),
            ("B", 2),
            ("C", 3),
            ("D", 4),
            ("E", 5),
            ("F", 6),
            ("G", 7),
            ("H", 8),
        ]);
        let table = Table::new(&db);
        let root: Vec<u16> = (0..8).collect();
        let (total, mut cert) = solve(&table, &db, true, &root);
        let child = cert
            .entries
            .iter()
            .find(|((s, _), e)| s.len() >= 2 && s.len() < 8 && matches!(e, Entry::Finite { .. }))
            .map(|(k, _)| k.clone())
            .unwrap();
        cert.entries.insert(child.clone(), Entry::Infeasible);
        let mut checker = Checker::new(&table, &db, true, &cert).unwrap();
        assert!(!checker.infeasible(&child.0, &child.1));
        assert!(!checker.lower(&root, &[], total as u64 + 1));
    }

    #[test]
    fn rejects_tables_where_win_does_not_identify_the_guess() {
        // Two identical characters: guessing one gives all-correct feedback on the other.
        let db = synth(&[("A", 1), ("A", 1)]);
        let table = Table::new(&db);
        assert!(Checker::new(&table, &db, true, &Certificate::default()).is_err());
    }
}
