use crate::data::Character;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    Correct = 0,
    Low = 1,
    High = 2,
    Partial = 3,
    Wrong = 4,
}

impl Status {
    pub fn as_js(self) -> &'static str {
        match self {
            Self::Correct => "correct",
            Self::Low => "low",
            Self::High => "high",
            Self::Partial => "partial",
            Self::Wrong => "wrong",
        }
    }
}

pub type Feedback = u32;
pub const WIN: Feedback = 0;

fn eq(ok: bool) -> Status {
    if ok { Status::Correct } else { Status::Wrong }
}

/// `compareNum` in gameLogic.js.
fn compare(guess: f64, target: f64) -> Status {
    if guess == target {
        Status::Correct
    } else if guess < target {
        Status::Low
    } else {
        Status::High
    }
}

/// Age/height: null vs null is correct, null vs value is wrong, else compareNum.
fn compare_nullable(guess: Option<f64>, target: Option<f64>) -> Status {
    match (guess, target) {
        (Some(g), Some(t)) => compare(g, t),
        (g, t) => eq(g.is_none() && t.is_none()),
    }
}

/// Studio/hair, same algorithm as the JS (length + membership, not set equality).
fn compare_multi(guess: &[String], target: &[String]) -> Status {
    if guess.len() == target.len() && guess.iter().all(|g| target.contains(g)) {
        Status::Correct
    } else if guess.iter().any(|g| target.contains(g)) {
        Status::Partial
    } else {
        Status::Wrong
    }
}

/// Port of `buildResult` in src/utils/gameLogic.js, in HEADERS order.
pub fn cells(g: &Character, t: &Character) -> [Status; 9] {
    [
        eq(g.name == t.name),
        eq(g.sex == t.sex),
        eq(g.seiyuu == t.seiyuu),
        eq(g.anime.name == t.anime.name),
        compare(g.anime.year, t.anime.year),
        compare_multi(g.anime.studio.as_slice(), t.anime.studio.as_slice()),
        compare_nullable(g.age, t.age),
        compare_nullable(g.height, t.height),
        compare_multi(g.hair_color.as_slice(), t.hair_color.as_slice()),
    ]
}

pub fn encode(cells: &[Status; 9]) -> Feedback {
    cells.iter().fold(0, |acc, &s| acc * 5 + s as Feedback)
}

pub fn feedback(guess: &Character, target: &Character) -> Feedback {
    encode(&cells(guess, target))
}

/// Feedback for every (guess, target) pair, row-major by guess.
pub struct Table {
    n: usize,
    codes: Vec<Feedback>,
}

impl Table {
    pub fn new(db: &[Character]) -> Self {
        let codes = db
            .iter()
            .flat_map(|g| db.iter().map(move |t| feedback(g, t)))
            .collect();
        Self { n: db.len(), codes }
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    pub fn get(&self, guess: usize, target: usize) -> Feedback {
        self.codes[guess * self.n + target]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::parse;

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

    #[test]
    fn win_iff_same_character_and_feedback_separates_all_pairs() {
        let db = repo_db();
        let table = Table::new(&db);
        for t in 0..db.len() {
            for g in 0..db.len() {
                assert_eq!(
                    table.get(g, t) == WIN,
                    g == t,
                    "{} vs {}",
                    db[g].name,
                    db[t].name
                );
            }
        }
        // No two targets are indistinguishable under every guess (the solver relies on this to always finish).
        for a in 0..db.len() {
            for b in a + 1..db.len() {
                assert!(
                    (0..db.len()).any(|g| table.get(g, a) != table.get(g, b)),
                    "{} / {}",
                    db[a].name,
                    db[b].name
                );
            }
        }
    }

    #[test]
    fn same_name_different_character_is_not_a_win() {
        let db = repo_db();
        let a = db.iter().position(|c| c.name == "Kagura").unwrap();
        let b = db.iter().rposition(|c| c.name == "Kagura").unwrap();
        assert_ne!(a, b);
        assert_eq!(cells(&db[b], &db[a])[0], Status::Correct);
        assert_ne!(feedback(&db[b], &db[a]), WIN);
    }

    #[test]
    fn encode_is_injective() {
        use Status::*;
        let all = [Correct, Low, High, Partial, Wrong];
        let mut seen = std::collections::HashSet::new();
        for a in all {
            for b in all {
                for c in all {
                    assert!(seen.insert(encode(&[
                        a, b, c, Correct, Correct, Correct, Correct, Correct, Correct
                    ])));
                }
            }
        }
    }
}
