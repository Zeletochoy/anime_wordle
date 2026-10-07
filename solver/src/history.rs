use crate::data::{Character, find_by_name};
use crate::solver::Set;
use chrono::NaiveDate;

pub struct PastPick {
    pub day: NaiveDate,
    pub character: usize,
}

pub struct HistoryRule {
    pub character_days: i64,
    pub anime_days: i64,
}

impl Default for HistoryRule {
    fn default() -> Self {
        Self {
            character_days: 90,
            anime_days: 30,
        }
    }
}

pub fn resolve(db: &[Character], rows: &[(NaiveDate, String)]) -> (Vec<PastPick>, Vec<String>) {
    let mut picks = Vec::new();
    let mut unknown = Vec::new();
    for (day, name) in rows {
        match find_by_name(db, name) {
            Some(character) => picks.push(PastPick {
                day: *day,
                character,
            }),
            None => unknown.push(name.clone()),
        }
    }
    (picks, unknown)
}

pub fn support(db: &[Character], past: &[PastPick], today: NaiveDate, rule: &HistoryRule) -> Set {
    let recent = |p: &&PastPick, days: i64| (1..days).contains(&(today - p.day).num_days());
    (0..db.len())
        .filter(|&i| {
            !past.iter().any(|p| {
                (p.character == i && recent(&p, rule.character_days))
                    || (db[p.character].anime.name == db[i].anime.name
                        && recent(&p, rule.anime_days))
            })
        })
        .map(|i| i as u16)
        .collect()
}

pub struct Gaps {
    pub picks: usize,
    pub min_character_gap: Option<i64>,
    pub min_anime_gap: Option<i64>,
}

pub fn min_gaps(db: &[Character], past: &[PastPick], since: NaiveDate) -> Gaps {
    let recent: Vec<&PastPick> = past.iter().filter(|p| p.day >= since).collect();
    let min = |acc: Option<i64>, gap: i64| Some(acc.map_or(gap, |g| g.min(gap)));
    let mut gaps = Gaps {
        picks: recent.len(),
        min_character_gap: None,
        min_anime_gap: None,
    };
    for (i, a) in recent.iter().enumerate() {
        for b in &recent[i + 1..] {
            let gap = (b.day - a.day).num_days().abs();
            if a.character == b.character {
                gaps.min_character_gap = min(gaps.min_character_gap, gap);
            }
            if db[a.character].anime.name == db[b.character].anime.name {
                gaps.min_anime_gap = min(gaps.min_anime_gap, gap);
            }
        }
    }
    gaps
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::parse;

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    fn db() -> Vec<Character> {
        let row = |name: &str, anime: &str| {
            format!(
                r#"{{"name":"{name}","anime":{{"name":"{anime}","year":2000,"studio":"S"}},"age":1,"height":1,"hair_color":"R","sex":null,"seiyuu":"V"}}"#
            )
        };
        parse(&format!(
            "[{},{},{},{}]",
            row("A1", "A"),
            row("A2", "A"),
            row("B1", "B"),
            row("C1", "C")
        ))
        .unwrap()
    }

    #[test]
    fn excludes_recent_anime_and_character() {
        // A1 picked 10 days ago → A1, A2 out (anime). B1 picked 40 days ago → B1 out (character), anime B free.
        let past = [
            PastPick {
                day: d("2026-09-27"),
                character: 0,
            },
            PastPick {
                day: d("2026-08-28"),
                character: 2,
            },
        ];
        assert_eq!(
            support(&db(), &past, d("2026-10-07"), &HistoryRule::default()),
            [3]
        );
    }

    #[test]
    fn window_boundaries() {
        let at = |days_ago: u64| {
            support(
                &db(),
                &[PastPick {
                    day: d("2026-10-07") - chrono::Days::new(days_ago),
                    character: 0,
                }],
                d("2026-10-07"),
                &HistoryRule::default(),
            )
        };
        assert!(!at(29).contains(&1));
        assert!(at(30).contains(&1));
        assert!(!at(89).contains(&0));
        assert!(at(90).contains(&0));
    }

    #[test]
    fn ignores_today_and_future_picks() {
        let past = [
            PastPick {
                day: d("2026-10-07"),
                character: 0,
            },
            PastPick {
                day: d("2026-10-08"),
                character: 2,
            },
        ];
        assert_eq!(
            support(&db(), &past, d("2026-10-07"), &HistoryRule::default()),
            [0, 1, 2, 3]
        );
    }

    #[test]
    fn resolve_reports_unknown_names() {
        let (picks, unknown) = resolve(
            &db(),
            &[
                (d("2026-10-01"), "B1".into()),
                (d("2026-10-02"), "Gone".into()),
            ],
        );
        assert_eq!((picks.len(), picks[0].character), (1, 2));
        assert_eq!(unknown, ["Gone"]);
    }

    #[test]
    fn min_gaps_since_date() {
        let past = [
            PastPick {
                day: d("2026-06-01"),
                character: 0,
            },
            PastPick {
                day: d("2026-06-03"),
                character: 0,
            }, // before `since`
            PastPick {
                day: d("2026-08-01"),
                character: 0,
            },
            PastPick {
                day: d("2026-09-05"),
                character: 1,
            },
            PastPick {
                day: d("2026-11-15"),
                character: 0,
            },
        ];
        let g = min_gaps(&db(), &past, d("2026-07-14"));
        assert_eq!(
            (g.picks, g.min_anime_gap, g.min_character_gap),
            (3, Some(35), Some(106))
        );
    }
}
