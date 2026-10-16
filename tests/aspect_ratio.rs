//! A flex or grid container with `aspect-ratio` takes its missing axis from the known one, as a
//! block container does.

use cephal::geometry::{AvailableSpace, Size};
use cephal::style::{Dimension, Display, LengthPercentageAuto};
use cephal::{Style, Tree};

const VIEWPORT: Size<AvailableSpace> =
    Size { width: AvailableSpace::Definite(800.0), height: AvailableSpace::Definite(600.0) };

/// A 300px block parent holding one container of `display` with `aspect-ratio: 1` and an auto
/// height, which holds one 20px tall child. Returns the container's size.
fn container_size(display: Display, configure: impl FnOnce(&mut Style)) -> Size<f32> {
    let mut tree: Tree<()> = Tree::new();
    let mut leaf = Style::DEFAULT;
    leaf.size.height = Dimension::length(20.0);
    let leaf = tree.new_leaf(leaf);
    let mut container = Style::DEFAULT;
    container.display = display;
    container.aspect_ratio = Some(1.0);
    configure(&mut container);
    let container = tree.new_with_children(container, &[leaf]);
    let mut parent = Style::DEFAULT;
    parent.size.width = Dimension::length(300.0);
    let parent = tree.new_with_children(parent, &[container]);
    tree.compute_layout(parent, VIEWPORT);
    tree.unrounded_layout(container).size
}

#[test]
fn a_stretched_container_takes_its_height_from_its_width() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        assert_eq!(container_size(display, |_| {}), Size { width: 300.0, height: 300.0 }, "{display:?}");
    }
}

#[test]
fn the_transferred_height_is_clamped_as_in_block_layout() {
    let clamp = |style: &mut Style| style.max_size.height = LengthPercentageAuto::length(120.0);
    let block = container_size(Display::Block, clamp);
    assert_eq!(block.height, 120.0);
    for display in [Display::Flex, Display::Grid] {
        assert_eq!(container_size(display, clamp), block, "{display:?}");
    }
}
