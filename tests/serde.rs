//! Styles survive a serde round trip unchanged.

mod common;

use cephal::Style;
use cephal::style::Position;
use common::{Rng, random_style};

fn round_trip(style: &Style) -> Style {
    let json = serde_json::to_string(style).expect("a style serializes");
    serde_json::from_str(&json).expect("a serialized style deserializes")
}

#[test]
fn random_styles_round_trip() {
    let mut rng = Rng(7);
    for i in 0..500 {
        let mut style = random_style(&mut rng, i % 2 == 0);
        if i % 7 == 0 {
            style.position = Position::Fixed;
        }
        assert_eq!(round_trip(&style), style, "style {i}");
    }
}

#[test]
fn the_default_style_round_trips() {
    assert_eq!(round_trip(&Style::DEFAULT), Style::DEFAULT);
}
