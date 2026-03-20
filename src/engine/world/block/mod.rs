pub mod builder;
pub mod definition;
pub mod flags;
pub mod id;
pub mod registry;
pub mod resolved;
pub mod textures;
pub mod tint;

use builder::BlockBuilder;
use registry::BlockRegistry;
use textures::BlockTextures;

pub fn create_default_block_registry() -> BlockRegistry {
    let mut registry = BlockRegistry::new();

    registry
        .register(BlockBuilder::new("air").replaceable().textures(BlockTextures::all("ui/empty")));

    registry.register(BlockBuilder::new("grass").solid().opaque().textures(
        BlockTextures::top_bottom_sides(
            "kenny/Tiles/grass_top",
            "kenny/Tiles/dirt",
            "kenny/Tiles/dirt_grass",
        ),
    ));

    registry.register(
        BlockBuilder::new("dirt").solid().opaque().textures(BlockTextures::all("kenny/Tiles/dirt")),
    );

    registry.register(
        BlockBuilder::new("stone")
            .solid()
            .opaque()
            .textures(BlockTextures::all("kenny/Tiles/stone")),
    );

    registry.register(
        BlockBuilder::new("oak_planks")
            .solid()
            .opaque()
            .textures(BlockTextures::all("kenny/Tiles/wood")),
    );

    registry.register(BlockBuilder::new("log").solid().opaque().textures(
        BlockTextures::top_bottom_sides(
            "kenny/Tiles/trunk_top",
            "kenny/Tiles/trunk_bottom",
            "kenny/Tiles/trunk_side",
        ),
    ));

    registry.register(
        BlockBuilder::new("glass")
            .solid()
            .transparent()
            .textures(BlockTextures::all("kenny/Tiles/glass_frame")),
    );

    registry.register(
        BlockBuilder::new("leaves")
            .solid()
            .transparent()
            .no_cull()
            .textures(BlockTextures::all("kenny/Tiles/leaves_transparent")),
    );

    registry.register(
        BlockBuilder::new("sand").solid().opaque().textures(BlockTextures::all("kenny/Tiles/sand")),
    );

    registry.register(
        BlockBuilder::new("water")
            .transparent()
            .liquid()
            .replaceable()
            .raycast_through()
            .textures(BlockTextures::all("kenny/Tiles/water")),
    );

    registry
}
