#[path = "../src/markdown/mod.rs"]
pub mod markdown;
use markdown::*;
use std::sync::atomic::AtomicBool;
fn parsed(text: &str) -> Document {
    parse(text, &AtomicBool::new(false)).unwrap()
}
fn flat(block: &Block) -> String {
    block.runs.iter().map(|r| r.text.as_str()).collect()
}

#[test]
fn empty_actual_release_highlights_are_supported() {
    assert!(parsed("\n\n").blocks.is_empty());
    assert_eq!(
        dispatch(&[SCHEMA], &AtomicBool::new(false)).unwrap(),
        [1, 0, 0, 0, 0]
    );
}
#[test]
fn generated_summary_and_details_preserve_unicode_headings_lists_and_emphasis() {
    let d = parsed(
        "## Summary\r\n#### 🔐 Security\r\n\r\n## Details\n### Changes\n- **Bold** and *italic*\n- `mcw` preserves café 你好 🦀\n",
    );
    assert_eq!(d.blocks.len(), 6);
    assert_eq!(d.blocks[0].kind, Kind::Heading(2));
    assert_eq!(flat(&d.blocks[1]), "🔐 Security");
    assert_eq!(d.blocks[4].kind, Kind::ListItem { depth: 0 });
    assert_eq!(
        d.blocks[4].runs,
        vec![
            Run {
                text: "Bold".into(),
                style: BOLD,
                link: None,
                title: None
            },
            Run {
                text: " and ".into(),
                style: 0,
                link: None,
                title: None
            },
            Run {
                text: "italic".into(),
                style: ITALIC,
                link: None,
                title: None
            }
        ]
    );
    assert_eq!(flat(&d.blocks[5]), "mcw preserves café 你好 🦀");
    assert_eq!(d.blocks[5].runs[0].style, CODE);
}
#[test]
fn list_numbering_nesting_continuation_and_quotes_are_typed() {
    let d =
        parsed("3. First\n9. Second\n  - Nested\n    continuation\n\n> Quoted\n> text\n\n---\n");
    assert_eq!(d.blocks[0].marker, "3.");
    assert_eq!(d.blocks[1].marker, "4.");
    assert_eq!(d.blocks[2].kind, Kind::ListItem { depth: 1 });
    assert_eq!(flat(&d.blocks[2]), "Nested continuation");
    assert_eq!(d.blocks[3].kind, Kind::Quote { depth: 1 });
    assert_eq!(flat(&d.blocks[3]), "Quoted text");
    assert_eq!(d.blocks[4].kind, Kind::Rule);
    let lazy = parsed("- first\nlazy continuation\n\nParagraph");
    assert_eq!(lazy.blocks.len(), 2);
    assert_eq!(flat(&lazy.blocks[0]), "first lazy continuation");
}
#[test]
fn links_reference_links_code_and_balanced_destinations_are_not_resource_fetches() {
    let d = parsed(
        "[**Source**](https://example.test/a_(b) \"Details\") and [Release][r].\n\n[r]: https://example.test/releases\n\n<https://example.test/>\n\n`` `raw` ``\n",
    );
    assert_eq!(d.blocks.len(), 3);
    assert_eq!(
        d.blocks[0].runs[0],
        Run {
            text: "Source".into(),
            style: BOLD,
            link: Some("https://example.test/a_(b)".into()),
            title: Some("Details".into())
        }
    );
    assert!(
        d.blocks[0]
            .runs
            .iter()
            .any(|r| r.text == "Release"
                && r.link.as_deref() == Some("https://example.test/releases"))
    );
    assert_eq!(flat(&d.blocks[2]), "`raw`");
    let linked_code = parsed("[`mcw`](https://example.test/)");
    assert_eq!(linked_code.blocks[0].runs[0].style, CODE);
    assert_eq!(
        linked_code.blocks[0].runs[0].link.as_deref(),
        Some("https://example.test/")
    );
}
#[test]
fn unsafe_links_and_html_cannot_activate_code_or_read_local_resources() {
    for destination in [
        "javascript:alert(1)",
        "file:///private/wallet.json",
        "data:text/html,hello",
        "https://user:password@example.test/",
        "\\\\server\\share",
        "https://",
    ] {
        let d = parsed(&format!("[label]({destination})"));
        assert!(
            d.blocks
                .iter()
                .flat_map(|b| &b.runs)
                .all(|r| r.link.is_none()),
            "{destination}"
        );
    }
    let d = parsed("<script>alert('x')</script>\n\n![image](https://example.test/image.png)\n");
    assert!(flat(&d.blocks[0]).contains("<script>"));
    assert_eq!(
        flat(&d.blocks[1]),
        "![image](https://example.test/image.png)"
    );
    assert!(
        d.blocks
            .iter()
            .flat_map(|b| &b.runs)
            .all(|r| r.link.is_none())
    );
}
#[test]
fn escapes_soft_hard_breaks_inline_code_and_entities_have_explicit_semantics() {
    let d = parsed(
        "A\\*literal\\* &amp; &#x1f980;\nsoft  \nhard\\\nbreak\n\n```rust\nlet x = \"<&>\";\n```\n\nName\n===\n",
    );
    assert_eq!(flat(&d.blocks[0]), "A*literal* & 🦀 soft\nhard\nbreak");
    assert_eq!(d.blocks[1].kind, Kind::Code);
    assert_eq!(d.blocks[1].marker, "rust");
    assert_eq!(flat(&d.blocks[1]), "let x = \"<&>\";");
    assert_eq!(d.blocks[2].kind, Kind::Heading(1));
    assert_eq!(flat(&parsed("a   b \t c `x  y`").blocks[0]), "a b c x  y");
}
#[test]
fn intraword_underscores_and_unclosed_delimiters_remain_literal() {
    assert_eq!(
        flat(&parsed("wallet_birth_height **unclosed").blocks[0]),
        "wallet_birth_height **unclosed"
    );
    assert_eq!(flat(&parsed("a `unclosed").blocks[0]), "a `unclosed");
}
#[test]
fn historical_safe_html_line_breaks_are_preserved_without_html_engine() {
    let d = parsed("#### Heading<br/>\n\nFirst<br>Second<BR />Third");
    assert_eq!(flat(&d.blocks[0]), "Heading\n");
    assert_eq!(flat(&d.blocks[1]), "First\nSecond\nThird");
}
#[test]
fn nested_strong_emphasis_and_strike_styles_compose() {
    let d = parsed("***both*** and **bold *inner* tail** and ~~gone~~");
    assert_eq!(d.blocks[0].runs[0].style, BOLD | ITALIC);
    assert!(
        d.blocks[0]
            .runs
            .iter()
            .any(|r| r.text == "inner" && r.style == BOLD | ITALIC)
    );
    assert!(
        d.blocks[0]
            .runs
            .iter()
            .any(|r| r.text == "gone" && r.style == STRIKE)
    );
    for (source, expected) in [
        ("*outer **inner** tail*", "outer inner tail"),
        ("**outer *inner***", "outer inner"),
        ("**outer `**` tail**", "outer ** tail"),
    ] {
        let d = parsed(source);
        assert_eq!(flat(&d.blocks[0]), expected, "{source}");
        assert!(
            d.blocks[0]
                .runs
                .iter()
                .any(|r| r.text == "inner" && r.style == BOLD | ITALIC)
                || d.blocks[0]
                    .runs
                    .iter()
                    .any(|r| r.text == "**" && r.style == BOLD | CODE),
            "{source}"
        );
    }
}
#[test]
fn cancelled_malformed_and_oversized_wire_requests_are_explicit() {
    assert_eq!(
        dispatch(&[], &AtomicBool::new(false)),
        Err(Error::InvalidInput)
    );
    assert_eq!(
        dispatch(&[2, b'x'], &AtomicBool::new(false)),
        Err(Error::InvalidInput)
    );
    assert_eq!(
        dispatch(&[1, 0xff], &AtomicBool::new(false)),
        Err(Error::InvalidInput)
    );
    assert_eq!(
        parse("secret\0", &AtomicBool::new(false)),
        Err(Error::InvalidInput)
    );
    assert_eq!(parse("x", &AtomicBool::new(true)), Err(Error::Cancelled));
    assert_eq!(
        parse(&"x".repeat(MAX_INPUT + 1), &AtomicBool::new(false)),
        Err(Error::Limit)
    );
    assert!(!Error::InvalidInput.diagnostic().contains("secret"));
    assert_eq!(
        parse(
            &"# heading\n".repeat(MAX_BLOCKS + 1),
            &AtomicBool::new(false)
        ),
        Err(Error::Limit)
    );
}
#[test]
fn adversarial_delimiter_input_is_bounded_and_never_panics() {
    for text in [
        "*[".repeat(10_000),
        "<".repeat(10_000),
        "[".repeat(10_000),
        "`x".repeat(10_000),
        "_".repeat(10_000),
        ">".repeat(100),
    ] {
        let result = parse(&text, &AtomicBool::new(false));
        assert!(result.is_ok() || matches!(result, Err(Error::Limit)));
    }
    let mut seed = 123456789_u32;
    for _ in 0..1000 {
        let mut text = String::new();
        for _ in 0..200 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let alphabet = b"abc[]()_*`~>\\\n \t\"<&;";
            text.push(char::from(alphabet[seed as usize % alphabet.len()]));
        }
        let result = parse(&text, &AtomicBool::new(false));
        assert!(result.is_ok() || matches!(result, Err(Error::Limit)));
    }
}
#[test]
fn late_angle_and_entity_terminators_exhaust_charged_work() {
    for (byte, terminator) in [("<", ">"), ("&", ";")] {
        let text = byte.repeat(MAX_INPUT - 1) + terminator;
        assert_eq!(parse(&text, &AtomicBool::new(false)), Err(Error::Limit));
        assert_eq!(parse(&text, &AtomicBool::new(true)), Err(Error::Cancelled));
    }
    let text = "[x](<".repeat(10_000);
    assert_eq!(parse(&text, &AtomicBool::new(false)), Err(Error::Limit));
}
#[test]
fn bounded_autolinks_keep_the_existing_length_and_unicode_boundaries() {
    let prefix = "https://example.test/";
    let approved = prefix.to_owned() + &"a".repeat(4096 - prefix.len());
    let document = parsed(&format!("<{approved}>"));
    assert_eq!(
        document.blocks[0].runs[0].link.as_deref(),
        Some(approved.as_str())
    );
    let unapproved = approved + "a";
    let document = parsed(&format!("<{unapproved}>"));
    assert!(document.blocks[0].runs.iter().all(|run| run.link.is_none()));
    let unicode = format!("<{}>", "🦀".repeat(1025));
    assert_eq!(flat(&parsed(&unicode).blocks[0]), unicode);
}
#[test]
fn serialized_output_preserves_utf8_and_document_count_with_no_platform_dependencies() {
    let d = parsed("## Café\n\n- 你好\n");
    let encoded = encode(&d).unwrap();
    assert_eq!(&encoded[..5], &[1, 2, 0, 0, 0]);
    assert!(encoded.windows(6).any(|w| w == "你好".as_bytes()));
    assert!(encoded.len() < MAX_OUTPUT);
}
