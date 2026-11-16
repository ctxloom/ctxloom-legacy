use std::time::Duration;

use super::parse;

// `now` is a span of no time, wherever a span is read.
#[test]
fn now_is_no_time() {
    assert_eq!(parse("now"), Ok(Duration::ZERO));
    assert_eq!(parse("  now "), Ok(Duration::ZERO));
}

// Anything else is duration-text's grammar, unchanged.
#[test]
fn a_span_is_duration_texts() {
    assert_eq!(parse("5m"), Ok(Duration::from_secs(300)));
    assert_eq!(parse("1h30m"), Ok(Duration::from_secs(5400)));
    assert_eq!(parse("0s"), Ok(Duration::ZERO));
    for bad in ["", "soon", "Now", "NOW", "5", "-5m", "nowish", "now 5m"] {
        assert!(parse(bad).is_err(), "{bad:?}");
    }
}
