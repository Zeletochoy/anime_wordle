use crate::feedback::{Feedback, Table};

/// Anything that answers guesses with feedback. The solver only ever sees this.
pub trait Game {
    fn guess(&mut self, guess: usize) -> Feedback;
}

/// Holds the secret target and answers guesses, nothing else.
/// The target is private and has no accessor: this is the no-cheating barrier.
pub struct Oracle<'a> {
    table: &'a Table,
    target: usize,
}

impl<'a> Oracle<'a> {
    pub fn new(table: &'a Table, target: usize) -> Self {
        Self { table, target }
    }
}

impl Game for Oracle<'_> {
    fn guess(&mut self, guess: usize) -> Feedback {
        self.table.get(guess, self.target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::parse;
    use crate::feedback::WIN;

    #[test]
    fn answers_with_the_target_feedback() {
        let db = parse(
            &std::fs::read_to_string(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../src/data/characters.json"
            ))
            .unwrap(),
        )
        .unwrap();
        let table = Table::new(&db);
        for (g, t) in [(0, 0), (3, 7), (100, 5), (42, 42)] {
            let fb = Oracle::new(&table, t).guess(g);
            assert_eq!(fb, table.get(g, t));
            assert_eq!(fb == WIN, g == t);
        }
    }
}
