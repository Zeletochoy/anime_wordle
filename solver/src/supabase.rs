use anyhow::{Context, bail, ensure};
use chrono::NaiveDate;
use serde::Deserialize;

const PAGE: usize = 1000;

#[derive(Deserialize)]
struct DailyRow {
    day: NaiveDate,
    character_name: String,
}

pub struct Supabase {
    http: reqwest::blocking::Client,
    url: String,
    key: String,
}

impl Supabase {
    pub fn new(url: &str, anon_key: &str) -> Self {
        Self {
            http: reqwest::blocking::Client::new(),
            url: url.trim_end_matches('/').to_owned(),
            key: anon_key.to_owned(),
        }
    }

    fn request(&self, method: reqwest::Method, table: &str) -> reqwest::blocking::RequestBuilder {
        self.http
            .request(method, format!("{}/rest/v1/{table}", self.url))
            .header("apikey", &self.key)
            .bearer_auth(&self.key)
    }

    fn daily_rows(&self, query: &[(&str, String)]) -> anyhow::Result<Vec<DailyRow>> {
        let mut q = vec![("select", "day,character_name".to_owned())];
        q.extend_from_slice(query);
        Ok(self
            .request(reqwest::Method::GET, "daily_index")
            .query(&q)
            .send()?
            .error_for_status()?
            .json()?)
    }

    /// Today's target name. Audited boundary: `Oracle::daily` must be the only caller.
    pub(crate) fn today_row(&self, today: NaiveDate) -> anyhow::Result<String> {
        let rows = self.daily_rows(&[("day", format!("eq.{today}"))])?;
        match rows.as_slice() {
            [row] if row.day == today => Ok(rows.into_iter().next().unwrap().character_name),
            _ => bail!(
                "expected exactly one daily_index row for {today}, got {}",
                rows.len()
            ),
        }
    }

    pub fn past_picks(&self, today: NaiveDate) -> anyhow::Result<Vec<(NaiveDate, String)>> {
        let mut out = Vec::new();
        loop {
            let rows = self.daily_rows(&[
                ("day", format!("lt.{today}")),
                ("order", "day.asc".to_owned()),
                ("limit", PAGE.to_string()),
                ("offset", out.len().to_string()),
            ])?;
            if let Some(r) = rows.iter().find(|r| r.day >= today) {
                bail!(
                    "history query returned a row for {} (today is {today}); refusing to use it",
                    r.day
                );
            }
            if rows.is_empty() {
                return Ok(out);
            }
            out.extend(rows.into_iter().map(|r| (r.day, r.character_name)));
        }
    }

    pub fn submit_score(
        &self,
        today: NaiveDate,
        username: &str,
        guesses: usize,
    ) -> anyhow::Result<()> {
        ensure!(
            !username.trim().is_empty(),
            "username is required to submit"
        );
        self.request(reqwest::Method::POST, "daily_scores")
            .json(&serde_json::json!({ "day": today, "username": username.trim(), "guesses": guesses, "hint_used": false }))
            .send()?
            .error_for_status()
            .context("score submission failed")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Matcher;

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    #[test]
    fn today_row_sends_auth_and_eq_filter() {
        let mut server = mockito::Server::new();
        let m = server
            .mock("GET", "/rest/v1/daily_index")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("select".into(), "day,character_name".into()),
                Matcher::UrlEncoded("day".into(), "eq.2026-10-07".into()),
            ]))
            .match_header("apikey", "k")
            .match_header("authorization", "Bearer k")
            .with_body(r#"[{"day":"2026-10-07","character_name":"Kagura"}]"#)
            .create();
        assert_eq!(
            Supabase::new(&server.url(), "k")
                .today_row(d("2026-10-07"))
                .unwrap(),
            "Kagura"
        );
        m.assert();
    }

    #[test]
    fn today_row_rejects_missing_extra_or_wrong_day_rows_without_leaking_names() {
        for body in [
            "[]",
            r#"[{"day":"2026-10-07","character_name":"Secret"},{"day":"2026-10-08","character_name":"Secret2"}]"#,
            r#"[{"day":"2026-10-08","character_name":"Secret"}]"#,
        ] {
            let mut server = mockito::Server::new();
            server
                .mock("GET", "/rest/v1/daily_index")
                .match_query(Matcher::Any)
                .with_body(body)
                .create();
            let err = Supabase::new(&server.url(), "k")
                .today_row(d("2026-10-07"))
                .unwrap_err()
                .to_string();
            assert!(!err.contains("Secret"), "{err}");
        }
    }

    #[test]
    fn past_picks_paginates_with_lt_filter_on_every_page() {
        let mut server = mockito::Server::new();
        let page = |from: usize, n: usize| {
            serde_json::to_string(&(from..from + n).map(|i| serde_json::json!({
            "day": (d("2020-01-01") + chrono::Days::new(i as u64)).to_string(), "character_name": format!("C{i}")
        })).collect::<Vec<_>>()).unwrap()
        };
        // A server cap below `limit` returns short pages; only an empty page ends the loop.
        for (offset, body) in [
            ("0", page(0, 500)),
            ("500", page(500, 503)),
            ("1003", "[]".to_owned()),
        ] {
            server
                .mock("GET", "/rest/v1/daily_index")
                .match_query(Matcher::AllOf(vec![
                    Matcher::UrlEncoded("day".into(), "lt.2026-10-07".into()),
                    Matcher::UrlEncoded("limit".into(), "1000".into()),
                    Matcher::UrlEncoded("offset".into(), offset.into()),
                ]))
                .with_body(body)
                .expect(1)
                .create();
        }
        assert_eq!(
            Supabase::new(&server.url(), "k")
                .past_picks(d("2026-10-07"))
                .unwrap()
                .len(),
            1003
        );
    }

    #[test]
    fn past_picks_rejects_today_and_future_rows() {
        for day in ["2026-10-07", "2026-10-08"] {
            let mut server = mockito::Server::new();
            server.mock("GET", "/rest/v1/daily_index").match_query(Matcher::UrlEncoded("offset".into(), "0".into()))
                .with_body(format!(r#"[{{"day":"2026-10-05","character_name":"A"}},{{"day":"{day}","character_name":"B"}}]"#)).create();
            assert!(
                Supabase::new(&server.url(), "k")
                    .past_picks(d("2026-10-07"))
                    .is_err()
            );
        }
    }

    #[test]
    fn submit_posts_expected_body() {
        let mut server = mockito::Server::new();
        let m = server.mock("POST", "/rest/v1/daily_scores")
            .match_body(Matcher::Json(serde_json::json!({"day":"2026-10-07","username":"solver-bot","guesses":3,"hint_used":false})))
            .with_status(201).create();
        Supabase::new(&server.url(), "k")
            .submit_score(d("2026-10-07"), "solver-bot", 3)
            .unwrap();
        m.assert();
    }
}
