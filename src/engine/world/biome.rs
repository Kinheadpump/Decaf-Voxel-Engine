use std::{fs, sync::Arc};

use anyhow::{Context, bail};
use serde::Deserialize;

use crate::engine::world::block::{id::BlockId, registry::BlockRegistry};

const BIOME_LUT_AXIS: usize = 64;
const BIOME_LUT_CELL_COUNT: usize = BIOME_LUT_AXIS * BIOME_LUT_AXIS;
const MIN_BIOME_AREA: f32 = 0.06;
const MAX_SPECIFICITY_BIAS: f32 = 4.0;
const PRIORITY_BIAS_STEP: f32 = 0.25;

#[derive(Debug, Clone)]
pub struct BiomeTable {
    biomes: Vec<ResolvedBiome>,
    lookup: Box<[u16; BIOME_LUT_CELL_COUNT]>,
}

impl BiomeTable {
    pub fn load_from_file(path: &str, block_registry: &BlockRegistry) -> anyhow::Result<Self> {
        let source = fs::read_to_string(path)
            .with_context(|| format!("failed to read biome file '{path}'"))?;
        let definition: BiomeFile = toml::from_str(&source)
            .with_context(|| format!("failed to parse biome file '{path}'"))?;
        Self::from_definition(definition, block_registry)
    }

    #[cfg(test)]
    pub fn single(surface_block: BlockId, soil_block: BlockId, deep_block: BlockId) -> Self {
        let biome = ResolvedBiome {
            name: Arc::<str>::from("default"),
            priority: 0,
            temperature_min: 0.0,
            temperature_max: 1.0,
            humidity_min: 0.0,
            humidity_max: 1.0,
            surface_block,
            soil_block,
            deep_block,
            height_offset: 0.0,
            roughness_multiplier: 1.0,
        };

        Self { biomes: vec![biome], lookup: Box::new([0u16; BIOME_LUT_CELL_COUNT]) }
    }

    pub fn sample(&self, temperature: f32, humidity: f32) -> &ResolvedBiome {
        let temperature_index = quantize_unit(temperature);
        let humidity_index = quantize_unit(humidity);
        let biome_index = self.lookup[lut_index(temperature_index, humidity_index)] as usize;
        &self.biomes[biome_index]
    }

    pub fn sample_blended(
        &self,
        temperature: f32,
        humidity: f32,
        blend_radius: f32,
    ) -> BiomeBlendSample<'_> {
        let dominant = self.sample(temperature, humidity);
        let blend_radius = blend_radius.clamp(0.0, 0.5);
        let mut total_weight = 0.0;
        let mut height_offset = 0.0;
        let mut roughness_multiplier = 0.0;

        for biome in &self.biomes {
            let temperature_weight = smooth_range_weight(
                temperature,
                biome.temperature_min,
                biome.temperature_max,
                blend_radius,
            );
            let humidity_weight =
                smooth_range_weight(humidity, biome.humidity_min, biome.humidity_max, blend_radius);
            let range_weight = temperature_weight * humidity_weight;

            if range_weight <= f32::EPSILON {
                continue;
            }

            // Narrower and higher-priority biomes should influence the blend more strongly than
            // the all-covering fallback biome, otherwise border areas still "snap" toward fallback.
            let specificity_bias =
                (1.0 / biome.coverage_area().max(MIN_BIOME_AREA)).min(MAX_SPECIFICITY_BIAS);
            let priority_bias = 1.0 + biome.priority.max(0) as f32 * PRIORITY_BIAS_STEP;
            let weight = range_weight * specificity_bias * priority_bias;

            total_weight += weight;
            height_offset += biome.height_offset * weight;
            roughness_multiplier += biome.roughness_multiplier * weight;
        }

        if total_weight <= f32::EPSILON {
            return BiomeBlendSample {
                dominant,
                height_offset: dominant.height_offset,
                roughness_multiplier: dominant.roughness_multiplier,
            };
        }

        BiomeBlendSample {
            dominant,
            height_offset: height_offset / total_weight,
            roughness_multiplier: roughness_multiplier / total_weight,
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.biomes.len()
    }

    fn from_definition(
        definition: BiomeFile,
        block_registry: &BlockRegistry,
    ) -> anyhow::Result<Self> {
        if definition.biomes.is_empty() {
            bail!("biome file must define at least one biome");
        }

        if definition.biomes.len() > u16::MAX as usize {
            bail!("biome file defines too many biomes");
        }

        let mut biomes = Vec::with_capacity(definition.biomes.len());
        for biome in definition.biomes {
            biomes.push(ResolvedBiome::resolve(biome, block_registry)?);
        }

        let fallback_index = if let Some(name) = definition.fallback_biome.as_deref() {
            biomes
                .iter()
                .position(|biome| biome.name.as_ref() == name)
                .with_context(|| format!("fallback biome '{name}' was not defined"))?
        } else {
            0
        };

        let lookup = compile_lookup(&biomes, fallback_index);
        Ok(Self { biomes, lookup })
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedBiome {
    pub name: Arc<str>,
    pub surface_block: BlockId,
    pub soil_block: BlockId,
    pub deep_block: BlockId,
    pub height_offset: f32,
    pub roughness_multiplier: f32,
    priority: i32,
    temperature_min: f32,
    temperature_max: f32,
    humidity_min: f32,
    humidity_max: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct BiomeBlendSample<'a> {
    pub dominant: &'a ResolvedBiome,
    pub height_offset: f32,
    pub roughness_multiplier: f32,
}

impl ResolvedBiome {
    fn resolve(
        definition: BiomeDefinition,
        block_registry: &BlockRegistry,
    ) -> anyhow::Result<Self> {
        let temperature_min =
            definition.temperature_min.min(definition.temperature_max).clamp(0.0, 1.0);
        let temperature_max =
            definition.temperature_min.max(definition.temperature_max).clamp(0.0, 1.0);
        let humidity_min = definition.humidity_min.min(definition.humidity_max).clamp(0.0, 1.0);
        let humidity_max = definition.humidity_min.max(definition.humidity_max).clamp(0.0, 1.0);
        let roughness_multiplier = definition.roughness_multiplier.max(0.0);

        Ok(Self {
            name: Arc::<str>::from(definition.name),
            priority: definition.priority,
            temperature_min,
            temperature_max,
            humidity_min,
            humidity_max,
            surface_block: resolve_block(&definition.surface_block, block_registry)?,
            soil_block: resolve_block(&definition.soil_block, block_registry)?,
            deep_block: resolve_block(&definition.deep_block, block_registry)?,
            height_offset: definition.height_offset,
            roughness_multiplier,
        })
    }

    fn matches(&self, temperature: f32, humidity: f32) -> bool {
        self.temperature_min <= temperature
            && temperature <= self.temperature_max
            && self.humidity_min <= humidity
            && humidity <= self.humidity_max
    }

    fn coverage_area(&self) -> f32 {
        (self.temperature_max - self.temperature_min) * (self.humidity_max - self.humidity_min)
    }
}

#[derive(Debug, Deserialize)]
struct BiomeFile {
    #[serde(default)]
    fallback_biome: Option<String>,
    biomes: Vec<BiomeDefinition>,
}

#[derive(Debug, Deserialize)]
struct BiomeDefinition {
    name: String,
    #[serde(default)]
    priority: i32,
    temperature_min: f32,
    temperature_max: f32,
    humidity_min: f32,
    humidity_max: f32,
    surface_block: String,
    soil_block: String,
    deep_block: String,
    #[serde(default)]
    height_offset: f32,
    #[serde(default = "default_roughness_multiplier")]
    roughness_multiplier: f32,
}

fn default_roughness_multiplier() -> f32 {
    1.0
}

fn compile_lookup(
    biomes: &[ResolvedBiome],
    fallback_index: usize,
) -> Box<[u16; BIOME_LUT_CELL_COUNT]> {
    let mut lookup = Box::new([0u16; BIOME_LUT_CELL_COUNT]);

    for humidity_index in 0..BIOME_LUT_AXIS {
        let humidity = sample_point(humidity_index);

        for temperature_index in 0..BIOME_LUT_AXIS {
            let temperature = sample_point(temperature_index);
            let mut best_index = fallback_index;
            let mut best_priority = i32::MIN;
            let mut best_area = f32::INFINITY;

            for (index, biome) in biomes.iter().enumerate() {
                if !biome.matches(temperature, humidity) {
                    continue;
                }

                let area = biome.coverage_area();
                if biome.priority > best_priority
                    || (biome.priority == best_priority && area < best_area)
                {
                    best_index = index;
                    best_priority = biome.priority;
                    best_area = area;
                }
            }

            lookup[lut_index(temperature_index, humidity_index)] = best_index as u16;
        }
    }

    lookup
}

fn resolve_block(name: &str, block_registry: &BlockRegistry) -> anyhow::Result<BlockId> {
    block_registry.get_id(name).with_context(|| format!("biome referenced unknown block '{name}'"))
}

fn smooth_range_weight(value: f32, min: f32, max: f32, blend_radius: f32) -> f32 {
    if blend_radius <= f32::EPSILON {
        return ((min <= value) && (value <= max)) as u8 as f32;
    }

    let half_width = ((max - min) * 0.5).max(0.0);
    let edge = blend_radius.min(half_width.max(0.001));
    let inner_min = min + edge;
    let inner_max = max - edge;
    let rise = smoothstep_range(min - edge, inner_min, value);
    let fall = 1.0 - smoothstep_range(inner_max, max + edge, value);

    rise.min(fall).clamp(0.0, 1.0)
}

#[inline]
fn smoothstep_range(edge0: f32, edge1: f32, value: f32) -> f32 {
    if (edge1 - edge0).abs() <= f32::EPSILON {
        return (value >= edge1) as u8 as f32;
    }

    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[inline]
fn quantize_unit(value: f32) -> usize {
    let scaled = value.clamp(0.0, 1.0) * (BIOME_LUT_AXIS - 1) as f32;
    scaled.round() as usize
}

#[inline]
fn sample_point(index: usize) -> f32 {
    (index as f32 + 0.5) / BIOME_LUT_AXIS as f32
}

#[inline]
fn lut_index(temperature_index: usize, humidity_index: usize) -> usize {
    temperature_index + humidity_index * BIOME_LUT_AXIS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::world::block::create_default_block_registry;

    fn biome_table_from_toml(source: &str) -> BiomeTable {
        let registry = create_default_block_registry();
        let definition: BiomeFile = toml::from_str(source).expect("biome definition should parse");
        BiomeTable::from_definition(definition, &registry).expect("biome definition should load")
    }

    #[test]
    fn biome_table_uses_priority_for_overlapping_ranges() {
        let table = biome_table_from_toml(
            r#"
fallback_biome = "temperate"

[[biomes]]
name = "temperate"
priority = 0
temperature_min = 0.0
temperature_max = 1.0
humidity_min = 0.0
humidity_max = 1.0
surface_block = "grass"
soil_block = "dirt"
deep_block = "stone"

[[biomes]]
name = "desert"
priority = 10
temperature_min = 0.7
temperature_max = 1.0
humidity_min = 0.0
humidity_max = 0.3
surface_block = "dirt"
soil_block = "dirt"
deep_block = "stone"
"#,
        );

        assert_eq!(table.len(), 2);
        assert_eq!(table.sample(0.9, 0.1).name.as_ref(), "desert");
        assert_eq!(table.sample(0.5, 0.5).name.as_ref(), "temperate");
    }

    #[test]
    fn biome_table_uses_fallback_when_no_specific_biome_matches() {
        let table = biome_table_from_toml(
            r#"
fallback_biome = "fallback"

[[biomes]]
name = "fallback"
priority = 0
temperature_min = 0.2
temperature_max = 0.8
humidity_min = 0.2
humidity_max = 0.8
surface_block = "grass"
soil_block = "dirt"
deep_block = "stone"

[[biomes]]
name = "cold_dry"
priority = 5
temperature_min = 0.0
temperature_max = 0.2
humidity_min = 0.0
humidity_max = 0.2
surface_block = "stone"
soil_block = "stone"
deep_block = "stone"
"#,
        );

        assert_eq!(table.sample(0.95, 0.95).name.as_ref(), "fallback");
        assert_eq!(table.sample(0.1, 0.1).name.as_ref(), "cold_dry");
    }

    #[test]
    fn biome_table_blends_numeric_modifiers_near_boundaries() {
        let table = biome_table_from_toml(
            r#"
fallback_biome = "temperate"

[[biomes]]
name = "temperate"
priority = 0
temperature_min = 0.0
temperature_max = 1.0
humidity_min = 0.0
humidity_max = 1.0
surface_block = "grass"
soil_block = "dirt"
deep_block = "stone"
height_offset = 0.0
roughness_multiplier = 1.0

[[biomes]]
name = "dry"
priority = 10
temperature_min = 0.5
temperature_max = 1.0
humidity_min = 0.0
humidity_max = 0.3
surface_block = "dirt"
soil_block = "dirt"
deep_block = "stone"
height_offset = 12.0
roughness_multiplier = 2.0
"#,
        );

        let blend = table.sample_blended(0.6, 0.28, 0.10);
        assert_eq!(blend.dominant.name.as_ref(), "dry");
        assert!(blend.height_offset > 0.0 && blend.height_offset < 12.0);
        assert!(blend.roughness_multiplier > 1.0 && blend.roughness_multiplier < 2.0);
    }
}
