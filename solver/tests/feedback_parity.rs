use anime_wordle_solver::{data, feedback};
use std::process::Command;

#[test]
fn rust_feedback_matches_js_build_result_for_all_pairs() {
    let root = env!("CARGO_MANIFEST_DIR");
    let db_path = format!("{root}/../src/data/characters.json");
    let db = data::parse(&std::fs::read_to_string(&db_path).unwrap()).unwrap();
    let out = Command::new("node")
        .arg(format!("{root}/tests/js_feedback.mjs"))
        .arg(&db_path)
        .output()
        .expect("node must be installed");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let js = String::from_utf8(out.stdout).unwrap();
    let mut lines = js.lines();
    for g in &db {
        for t in &db {
            let rust: Vec<_> = feedback::cells(g, t).iter().map(|s| s.as_js()).collect();
            assert_eq!(
                rust.join(","),
                lines.next().unwrap(),
                "guess {} / target {}",
                g.name,
                t.name
            );
        }
    }
    assert!(lines.next().is_none());
}
