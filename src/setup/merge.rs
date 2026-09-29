//! Adding a fragment to config.toml without losing a comment.
//!
//! `toml` reads and writes values and throws the comments away, which on a
//! file somebody annotated by hand is most of what they wrote. `toml_edit`
//! keeps the document as it was typed, so a merge here changes the keys it
//! names and leaves every other byte where it was.
//!
//! The rules are the ones a person would follow by hand: a table the file has
//! gets the fragment's keys written into it, a table it lacks is added, a
//! value the fragment names is set to the fragment's, and an array of tables
//! gets an entry only when none of that `name` is there already.

use toml_edit::{DocumentMut, Item, Table};

/// Merge a fragment into a document.
///
/// Whatever is added goes at the end of the file. A table carries the
/// position it had in the document it was parsed from, and a table from the
/// fragment keeps the fragment's, so without renumbering a new `[[layout]]`
/// was written between the file's own `[[layout]]` and its windows, and took
/// them.
pub fn merge(doc: &mut DocumentMut, fragment: &DocumentMut) {
    let mut next = last_position(doc.as_table()) + 1;
    merge_table(doc.as_table_mut(), fragment.as_table(), &mut next);
}

/// The highest position of any table in a table, itself included.
fn last_position(table: &Table) -> isize {
    let mut last = table.position().unwrap_or(0);
    for (_, item) in table.iter() {
        match item {
            Item::Table(t) => last = last.max(last_position(t)),
            Item::ArrayOfTables(a) => {
                for t in a.iter() {
                    last = last.max(last_position(t));
                }
            }
            _ => {}
        }
    }
    last
}

/// Give a table and every table in it the next positions, in reading order,
/// and a blank line above each, the way a person would leave one.
fn renumber(table: &mut Table, next: &mut isize) {
    table.set_position(Some(*next));
    table.decor_mut().set_prefix("\n");
    *next += 1;
    for (_, item) in table.iter_mut() {
        renumber_item(item, next);
    }
}

fn renumber_item(item: &mut Item, next: &mut isize) {
    match item {
        Item::Table(t) => renumber(t, next),
        Item::ArrayOfTables(a) => {
            for t in a.iter_mut() {
                renumber(t, next);
            }
        }
        _ => {}
    }
}

fn merge_table(into: &mut Table, from: &Table, next: &mut isize) {
    for (key, item) in from.iter() {
        match (into.get_mut(key), item) {
            (Some(Item::Table(ours)), Item::Table(theirs)) => merge_table(ours, theirs, next),
            (Some(Item::ArrayOfTables(ours)), Item::ArrayOfTables(theirs)) => {
                for t in theirs.iter() {
                    let name = t.get("name").and_then(|n| n.as_str());
                    let present = name.is_some_and(|n| {
                        ours.iter()
                            .any(|o| o.get("name").and_then(|v| v.as_str()) == Some(n))
                    });
                    if !present {
                        let mut t = t.clone();
                        renumber(&mut t, next);
                        ours.push(t);
                    }
                }
            }
            (Some(Item::Value(ours)), Item::Value(theirs)) => {
                // The value changes and the comments around it stay.
                let decor = ours.decor().clone();
                *ours = theirs.clone();
                *ours.decor_mut() = decor;
            }
            (_, item) => {
                let mut item = item.clone();
                renumber_item(&mut item, next);
                into.insert(key, item);
            }
        }
    }
}

/// Whether a dotted path is in a document.
pub fn has(doc: &DocumentMut, path: &str) -> bool {
    let mut item = doc.as_item();
    for part in path.split('.') {
        match item.get(part) {
            Some(next) => item = next,
            None => return false,
        }
    }
    true
}

/// The file with a fragment merged in.
///
/// `seed` is what is in force for an array the fragment adds to, written first
/// when the file does not have that array: `[[status.right.segments]]` in a
/// file replaces the whole default list, so adding `agents` to a file that
/// never listed segments would otherwise take git, the network and the
/// battery off the bar.
pub fn merged(text: &str, fragment: &str, seed: Option<(&str, &str)>) -> anyhow::Result<String> {
    let doc: DocumentMut = text.parse()?;
    // A file of nothing but comments keeps them in the document's trailing
    // text, which is written after every table, so a merge would move a
    // header comment to the bottom. Such a file is kept as it is and the
    // merge is written after it.
    if doc.as_table().is_empty() {
        let added = merged_into(DocumentMut::new(), fragment, seed)?;
        let text = text.trim_end();
        return Ok(if text.is_empty() {
            added.trim_start().to_string()
        } else {
            format!("{text}\n{added}")
        });
    }
    merged_into(doc, fragment, seed)
}

fn merged_into(
    mut doc: DocumentMut,
    fragment: &str,
    seed: Option<(&str, &str)>,
) -> anyhow::Result<String> {
    if let Some((path, seed_text)) = seed
        && !has(&doc, path)
    {
        let seed_doc: DocumentMut = seed_text.parse()?;
        merge(&mut doc, &seed_doc);
    }
    let fragment: DocumentMut = fragment.parse()?;
    merge(&mut doc, &fragment);
    Ok(doc.to_string())
}

/// The segments in force, as the TOML that would write them.
///
/// A key whose value is the empty default is left out, so the file gets the
/// three lines a person would have written rather than nine.
pub fn segments_seed(config: &crate::config::Config) -> String {
    let quoted = |s: &str| toml::Value::String(s.to_string()).to_string();
    let mut out = String::new();
    for s in &config.status.right.segments {
        out.push_str("[[status.right.segments]]\n");
        out.push_str(&format!("name = {}\n", quoted(s.name.range_name())));
        if !s.separator_before.is_empty() {
            out.push_str(&format!(
                "separator_before = {}\n",
                quoted(&s.separator_before)
            ));
        }
        if !s.on_click.is_empty() {
            out.push_str(&format!("on_click = {}\n", quoted(&s.on_click)));
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_goes_into_the_table_that_is_there_and_every_comment_stays() {
        let before = "\
# my settings, do not lose this
[online]
# why I changed the probe
probe = \"9.9.9.9:53\" # quad nine

[journal]
enabled = true
";
        let after = merged(before, "[online]\nenabled = true", None).unwrap();
        assert!(after.contains("# my settings, do not lose this"), "{after}");
        assert!(after.contains("# why I changed the probe"), "{after}");
        assert!(
            after.contains("probe = \"9.9.9.9:53\" # quad nine"),
            "{after}"
        );
        assert!(after.contains("enabled = true"), "{after}");
        assert_eq!(after.matches("[online]").count(), 1, "{after}");
        let parsed = crate::config::parse(&after, std::path::Path::new("x")).unwrap();
        assert!(parsed.online.enabled);
        assert_eq!(parsed.online.probe, "9.9.9.9:53");
        assert!(parsed.journal.enabled);
    }

    #[test]
    fn a_value_written_off_is_turned_on_in_place_with_its_comment() {
        let before = "[notify]\nenabled = false # too noisy last week\n";
        let after = merged(before, "[notify]\nenabled = true", None).unwrap();
        assert_eq!(after, "[notify]\nenabled = true # too noisy last week\n");
    }

    #[test]
    fn a_table_the_file_lacks_is_added_and_a_nested_one_too() {
        let before = "# top\n[git]\nbranch_max_len = 30\n";
        let after = merged(before, "[git.autofetch]\nenabled = true", None).unwrap();
        assert!(
            after.starts_with("# top\n[git]\nbranch_max_len = 30\n"),
            "{after}"
        );
        let parsed = crate::config::parse(&after, std::path::Path::new("x")).unwrap();
        assert!(parsed.git.autofetch.enabled);
        assert_eq!(parsed.git.branch_max_len, 30);

        let after = merged("", "[window_names]\nenabled = true", None).unwrap();
        let parsed = crate::config::parse(&after, std::path::Path::new("x")).unwrap();
        assert!(parsed.window_names.enabled);
    }

    #[test]
    fn an_array_entry_is_added_once_and_the_defaults_are_kept() {
        let config = crate::config::Config::default();
        let seed = segments_seed(&config);
        let fragment = "[[status.right.segments]]\nname = \"agents\"";
        let after = merged("# mine\n", fragment, Some(("status.right.segments", &seed))).unwrap();
        let parsed = crate::config::parse(&after, std::path::Path::new("x")).unwrap();
        let names: Vec<_> = parsed
            .status
            .right
            .segments
            .iter()
            .map(|s| s.name.range_name())
            .collect();
        assert_eq!(names, vec!["git", "net", "battery", "agents"]);
        // The battery's separator survived the round trip byte for byte.
        assert_eq!(
            parsed.status.right.segments[2].separator_before,
            config.status.right.segments[2].separator_before
        );
        assert!(after.starts_with("# mine\n"), "{after}");
        assert!(!after.contains("on_click = \"\""), "{after}");
        // Again: nothing is added twice, and the seed is not written over a
        // list the file already has.
        let again = merged(&after, fragment, Some(("status.right.segments", &seed))).unwrap();
        assert_eq!(again, after);
        let health = merged(
            &after,
            "[[status.right.segments]]\nname = \"health\"",
            Some(("status.right.segments", &seed)),
        )
        .unwrap();
        let parsed = crate::config::parse(&health, std::path::Path::new("x")).unwrap();
        assert_eq!(parsed.status.right.segments.len(), 5);
    }

    #[test]
    fn a_layout_is_added_beside_one_of_another_name() {
        let before = "[[layout]]\nname = \"mine\"\n\n[[layout.window]]\nname = \"a\"\n";
        let fragment = "[[layout]]\nname = \"default\"\n\n[[layout.window]]\nname = \"edit\"\ncommand = \"nvim\"";
        let after = merged(before, fragment, None).unwrap();
        // Appended with a blank line above each new header, the file's own
        // lines as they were.
        assert!(after.starts_with(before), "{after}");
        assert!(
            after.contains("\n\n[[layout]]\nname = \"default\""),
            "{after}"
        );
        let parsed = crate::config::parse(&after, std::path::Path::new("x")).unwrap();
        let names: Vec<_> = parsed.layout.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, vec!["mine", "default"]);
        assert_eq!(parsed.layout[1].window[0].command, "nvim");
    }

    #[test]
    fn a_file_that_is_not_toml_is_refused_rather_than_rewritten() {
        assert!(merged("[online\n", "[online]\nenabled = true", None).is_err());
    }
}
