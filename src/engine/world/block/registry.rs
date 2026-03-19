use std::collections::HashMap;

use crate::engine::world::block::{
    builder::BlockBuilder, definition::BlockDefinition, id::BlockId,
};

#[derive(Default, Clone)]
pub struct BlockRegistry {
    blocks: Vec<BlockDefinition>,
    name_to_id: HashMap<String, BlockId>,
}

impl BlockRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, builder: BlockBuilder) -> BlockId {
        let id = BlockId(self.blocks.len() as u16);
        let def = builder.build(id);
        assert!(
            !self.name_to_id.contains_key(&def.name),
            "block '{}' was registered more than once",
            def.name
        );
        self.name_to_id.insert(def.name.clone(), id);
        self.blocks.push(def);
        id
    }

    pub fn get_id(&self, name: &str) -> Option<BlockId> {
        self.name_to_id.get(name).copied()
    }

    pub fn must_get_id(&self, name: &str) -> BlockId {
        self.get_id(name).unwrap_or_else(|| panic!("block '{name}' was not registered"))
    }

    pub fn get(&self, id: BlockId) -> Option<&BlockDefinition> {
        self.blocks.get(id.0 as usize)
    }

    pub fn iter(&self) -> impl Iterator<Item = &BlockDefinition> {
        self.blocks.iter()
    }

    pub fn len(&self) -> usize {
        self.blocks.len()
    }
}
