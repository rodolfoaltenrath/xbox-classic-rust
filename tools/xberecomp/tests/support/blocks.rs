use std::path::Path;

pub(super) fn sources(output: &Path) -> String {
    let mut straight = [0x01, 0xC8].repeat(1000);
    straight.push(0xC3);
    super::image(output, "blocks_straight", &straight);
    let mut branches = [0x01, 0xC8, 0xEB, 0].repeat(1000);
    branches.push(0xC3);
    super::image(output, "blocks_many", &branches);
    include_str!("../verification_blocks.inc").to_owned()
}
