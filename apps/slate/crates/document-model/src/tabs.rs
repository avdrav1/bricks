//! Tab stops (ADR 0009): where a paragraph's tabs advance to.
//!
//! A style's stops and a paragraph's own stops (Word's `w:tabs`) are layers: each layer's
//! stops add to the ones below it, and a [`TabAlign::Clear`] stop removes the stop below
//! it at its position ([`merge_tabs`]). Past the last stop, tabs advance to multiples of
//! the document's default stop ([`DocxInfo::default_tab`](crate::DocxInfo)).

use serde::{Deserialize, Serialize};

/// A tab stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabStop {
    /// From the text's left edge (the page's margin, not the paragraph's indent), in twips.
    pub position: i32,
    pub align: TabAlign,
    #[serde(default, skip_serializing_if = "TabLeader::is_none")]
    pub leader: TabLeader,
}

/// How the text after a tab lines up at its stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TabAlign {
    /// The text starts at the stop.
    #[default]
    Left,
    /// The text centres on the stop.
    Center,
    /// The text ends at the stop.
    Right,
    /// The text's decimal point is at the stop.
    Decimal,
    /// Not a stop: removes the stop at its position that a layer below sets.
    Clear,
}

/// What fills the gap a tab leaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TabLeader {
    #[default]
    None,
    Dot,
    Hyphen,
    Underscore,
    /// A heavy line.
    Heavy,
    /// Dots at the middle of the line.
    MiddleDot,
}

impl TabLeader {
    pub fn is_none(&self) -> bool {
        *self == TabLeader::None
    }
}

/// Put the layer `over` on top of the stops `under`, keeping them in position order:
/// each of its stops replaces the one at its position, and a clear removes it. Clears stay
/// in the result, so the layers merge the same in any grouping; [`effective_tabs`] drops
/// them.
pub fn merge_tabs(under: &mut Vec<TabStop>, over: &[TabStop]) {
    for stop in over {
        under.retain(|s| s.position != stop.position);
        let at = under.partition_point(|s| s.position < stop.position);
        under.insert(at, *stop);
    }
}

/// The stops tabs advance to of `under` with the layer `over` on top: in position order,
/// without clears.
pub fn effective_tabs(under: &[TabStop], over: &[TabStop]) -> Vec<TabStop> {
    let mut stops = under.to_vec();
    merge_tabs(&mut stops, over);
    stops.retain(|s| s.align != TabAlign::Clear);
    stops
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stop(position: i32, align: TabAlign) -> TabStop {
        TabStop {
            position,
            align,
            leader: TabLeader::None,
        }
    }

    #[test]
    fn a_layer_adds_replaces_and_clears_stops_in_position_order() {
        let mut stops = vec![stop(720, TabAlign::Left), stop(4320, TabAlign::Center)];
        merge_tabs(
            &mut stops,
            &[
                stop(2880, TabAlign::Right),
                stop(4320, TabAlign::Decimal),
                stop(720, TabAlign::Clear),
            ],
        );
        assert_eq!(
            stops,
            [
                stop(720, TabAlign::Clear),
                stop(2880, TabAlign::Right),
                stop(4320, TabAlign::Decimal)
            ]
        );
        assert_eq!(
            effective_tabs(&stops, &[]),
            [stop(2880, TabAlign::Right), stop(4320, TabAlign::Decimal)]
        );
    }

    #[test]
    fn a_clear_kept_in_a_layer_removes_the_stop_of_a_layer_below_it() {
        // Base -> style (clears 720) -> paragraph: the style's clear applies to the base's
        // stop though the style was resolved without it.
        let mut style = Vec::new();
        merge_tabs(&mut style, &[stop(720, TabAlign::Clear)]);
        let mut base = vec![stop(720, TabAlign::Left)];
        merge_tabs(&mut base, &style);
        assert_eq!(
            effective_tabs(&base, &[stop(1440, TabAlign::Left)]),
            [stop(1440, TabAlign::Left)]
        );
    }
}
