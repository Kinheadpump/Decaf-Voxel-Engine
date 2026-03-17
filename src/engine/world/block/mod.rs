pub mod builder;
pub mod definition;
pub mod flags;
pub mod id;
pub mod registry;
pub mod resolved;
pub mod textures;

use builder::BlockBuilder;
use registry::BlockRegistry;
use textures::BlockTextures;

pub fn create_default_block_registry() -> BlockRegistry {
    let mut reg = BlockRegistry::new();

    reg.register(BlockBuilder::new("air").replaceable().textures(BlockTextures::all("air")));

    reg.register(
        BlockBuilder::new("grass").solid().opaque().textures(BlockTextures::top_bottom_sides(
            "grass_top",
            "dirt",
            "grass_side",
        )),
    );

    reg.register(BlockBuilder::new("dirt").solid().opaque().textures(BlockTextures::all("dirt")));

    reg.register(BlockBuilder::new("stone").solid().opaque().textures(BlockTextures::all("stone")));

    reg.register(
        BlockBuilder::new("oak_planks").solid().opaque().textures(BlockTextures::all("oak_planks")),
    );

    reg.register(
        BlockBuilder::new("log")
            .solid()
            .opaque()
            .textures(BlockTextures::top_bottom_sides("log_top", "log_top", "log_side")),
    );

    reg.register(
        BlockBuilder::new("glass").solid().transparent().textures(BlockTextures::all("glass")),
    );

    reg.register(
        BlockBuilder::new("leaves")
            .solid()
            .transparent()
            .no_cull()
            .textures(BlockTextures::all("leaves")),
    );

    reg.register(
        BlockBuilder::new("water")
            .transparent()
            .liquid()
            .replaceable()
            .textures(BlockTextures::all("water")),
    );

    reg
}
