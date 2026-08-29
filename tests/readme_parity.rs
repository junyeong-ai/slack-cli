//! The Korean and English READMEs document one CLI, so the identifiers they
//! publish — commands, flags, environment variables, exit codes — must be the
//! same set. Prose is translated and headings are not comparable, but a table
//! row keyed by a code span names something in the binary, and a row present
//! in one language and missing from the other is a documentation gap.

const KOREAN: &str = include_str!("../README.md");
const ENGLISH: &str = include_str!("../README.en.md");

/// Lines outside fenced code blocks. A shell comment inside a fence starts
/// with `#` and a table-like line can appear in sample output, so neither the
/// heading nor the table scan may look at them.
fn prose_lines(readme: &str) -> impl Iterator<Item = &str> {
    let mut fenced = false;
    readme.lines().filter(move |line| {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            return false;
        }
        !fenced
    })
}

/// The first cell of every table row that names an identifier, in document
/// order. Header rows and prose-keyed rows carry translated text and are not
/// code spans, so they drop out on their own.
fn identifier_rows(readme: &str) -> Vec<&str> {
    prose_lines(readme)
        .filter(|line| line.starts_with("| "))
        .filter_map(|line| line.split('|').nth(1))
        .map(str::trim)
        .filter(|cell| cell.starts_with('`'))
        .collect()
}

#[test]
fn both_readmes_document_the_same_identifiers() {
    let korean = identifier_rows(KOREAN);
    let english = identifier_rows(ENGLISH);

    let missing_from_english: Vec<_> = korean.iter().filter(|row| !english.contains(row)).collect();
    let missing_from_korean: Vec<_> = english.iter().filter(|row| !korean.contains(row)).collect();

    assert!(
        missing_from_english.is_empty() && missing_from_korean.is_empty(),
        "README.md and README.en.md document different identifiers\n  \
         missing from README.en.md: {missing_from_english:?}\n  \
         missing from README.md:    {missing_from_korean:?}"
    );
    assert_eq!(
        korean, english,
        "README.md and README.en.md list the same identifiers in a different order"
    );
}

#[test]
fn the_readmes_carry_the_same_section_structure() {
    let sections = |readme: &str| {
        prose_lines(readme)
            .filter(|line| line.starts_with('#') && line.trim_start_matches('#').starts_with(' '))
            .map(|line| line.chars().take_while(|c| *c == '#').count())
            .collect::<Vec<_>>()
    };

    assert_eq!(
        sections(KOREAN),
        sections(ENGLISH),
        "README.md and README.en.md have diverged in section structure"
    );
}
