// SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
//
// SPDX-License-Identifier: MIT

#![allow(dead_code)]

use std::path::{Path, PathBuf};

pub fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures")
        .canonicalize()
        .expect("test/fixtures must exist")
}

/// Every `.css` file in the corpus, sorted by name.
pub fn fixtures() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(fixture_dir()).expect("readable fixture dir") {
        let path = entry.expect("readable entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("css") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("fixture {name} must be valid utf-8: {e}"));
        out.push((name, source));
    }
    out.sort();
    assert!(!out.is_empty(), "fixture corpus must not be empty");
    out
}

pub fn fixture(name: &str) -> String {
    std::fs::read_to_string(fixture_dir().join(name))
        .unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

/// Number of lines that differ between `before` and `after` -- added plus
/// removed -- via a proper LCS diff.
///
/// A common-prefix/suffix approximation would count everything between two
/// separate hunks as changed, which is exactly the failure mode these
/// diff-size assertions exist to catch, so it has to be a real diff.
pub fn changed_line_count(before: &str, after: &str) -> usize {
    let a: Vec<&str> = before.lines().collect();
    let b: Vec<&str> = after.lines().collect();

    // lcs[i][j] = length of the longest common subsequence of a[i..] and b[j..]
    let mut lcs = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let common = lcs[0][0];
    (a.len() - common) + (b.len() - common)
}

#[test]
fn changed_line_count_counts_a_single_edited_line_as_two() {
    assert_eq!(changed_line_count("a\nb\nc\n", "a\nX\nc\n"), 2);
}

#[test]
fn changed_line_count_is_zero_for_identical_input() {
    assert_eq!(changed_line_count("a\nb\n", "a\nb\n"), 0);
}

#[test]
fn changed_line_count_counts_a_pure_insertion_as_one() {
    assert_eq!(changed_line_count("a\nc\n", "a\nb\nc\n"), 1);
}

#[test]
fn changed_line_count_handles_two_separate_hunks() {
    // A prefix/suffix approximation would say 8 here.
    assert_eq!(changed_line_count("a\nb\nc\nd\ne\n", "X\nb\nc\nd\nY\n"), 4);
}

#[test]
fn changed_line_count_counts_a_pure_deletion() {
    assert_eq!(changed_line_count("a\nb\nc\n", "a\nc\n"), 1);
}
