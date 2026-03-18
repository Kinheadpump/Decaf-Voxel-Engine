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
    let mut registry = BlockRegistry::new();

    registry.register(BlockBuilder::new("air").replaceable().textures(BlockTextures::all("air")));

    registry.register(
        BlockBuilder::new("grass").solid().opaque().textures(BlockTextures::top_bottom_sides(
            "grass_top",
            "dirt",
            "grass_side",
        )),
    );

    registry
        .register(BlockBuilder::new("dirt").solid().opaque().textures(BlockTextures::all("dirt")));

    registry.register(
        BlockBuilder::new("stone").solid().opaque().textures(BlockTextures::all("stone")),
    );

    registry.register(
        BlockBuilder::new("oak_planks").solid().opaque().textures(BlockTextures::all("oak_planks")),
    );

    registry.register(
        BlockBuilder::new("log")
            .solid()
            .opaque()
            .textures(BlockTextures::top_bottom_sides("log_top", "log_top", "log_side")),
    );

    registry.register(
        BlockBuilder::new("glass").solid().transparent().textures(BlockTextures::all("glass")),
    );

    registry.register(
        BlockBuilder::new("leaves")
            .solid()
            .transparent()
            .no_cull()
            .textures(BlockTextures::all("leaves")),
    );

    registry.register(
        BlockBuilder::new("water")
            .transparent()
            .liquid()
            .replaceable()
            .raycast_through()
            .textures(BlockTextures::all("water")),
    );

    registry
}
