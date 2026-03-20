use std::{fs, sync::Arc};

use anyhow::{Context, bail};
use serde::Deserialize;

use crate::engine::world::block::{id::BlockId, registry::BlockRegistry};

const MIN_BIOME_COVERAGE: f32 = 0.06;
const MAX_SPECIFICITY_BIAS: f32 = 4.0;
const PRIORITY_BIAS_STEP: f32 = 0.25;
const DEFAULT_TINT_COLOR: [u8; 3] = [255, 255, 255];
const DEFAULT_ALTITUDE_SPAN: f32 = 256.0;

#[derive(Debug, Clone)]
pub struct BiomeTable {
    biomes: Vec<ResolvedBiome>,
    fallback_index: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BiomeSamplePoint {
    pub temperature: f32,
    pub humidity: f32,
    pub altitude: f32,
    pub continentalness: f32,
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
        Self::single_with_ocean_floor(surface_block, soil_block, deep_block, soil_block)
    }

    #[cfg(test)]
    pub fn single_with_ocean_floor(
        surface_block: BlockId,
        soil_block: BlockId,
        deep_block: BlockId,
        ocean_floor_block: BlockId,
    ) -> Self {
        let biome = ResolvedBiome {
            name: Arc::<str>::from("default"),
            priority: 0,
            temperature_min: 0.0,
            temperature_max: 1.0,
            humidity_min: 0.0,
            humidity_max: 1.0,
            altitude_constraint: RangeConstraint::unbounded(),
            continentalness_constraint: RangeConstraint::bounded(0.0, 1.0),
            surface_block,
            soil_block,
            deep_block,
            ocean_floor_block,
            grass_color: DEFAULT_TINT_COLOR,
            foliage_color: DEFAULT_TINT_COLOR,
            height_offset: 0.0,
            roughness_multiplier: 1.0,
        };

        Self { biomes: vec![biome], fallback_index: 0 }
    }

    pub fn sample(&self, point: BiomeSamplePoint) -> &ResolvedBiome {
        &self.biomes[self.select_biome_index(point)]
    }

    pub fn sample_blended(
        &self,
        point: BiomeSamplePoint,
        blend_radius: f32,
    ) -> BiomeBlendSample<'_> {
        let dominant = self.sample(point);
        let blend_radius = blend_radius.clamp(0.0, 0.5);
        let mut total_weight = 0.0;
        let mut height_offset = 0.0;
        let mut roughness_multiplier = 0.0;

        for biome in &self.biomes {
            let range_weight = biome.blend_weight(point, blend_radius);
            if range_weight <= f32::EPSILON {
                continue;
            }

            // Narrow, high-priority biomes should influence boundary blends more than the
            // all-covering fallback biome.
            let specificity_bias =
                (1.0 / biome.coverage_score().max(MIN_BIOME_COVERAGE)).min(MAX_SPECIFICITY_BIAS);
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

        Ok(Self { biomes, fallback_index })
    }

    fn select_biome_index(&self, point: BiomeSamplePoint) -> usize {
        let mut best_index = self.fallback_index;
        let mut best_priority = i32::MIN;
        let mut best_coverage = f32::INFINITY;

        for (index, biome) in self.biomes.iter().enumerate() {
            if !biome.matches(point) {
                continue;
            }

            let coverage = biome.coverage_score();
            if biome.priority > best_priority
                || (biome.priority == best_priority && coverage < best_coverage)
            {
                best_index = index;
                best_priority = biome.priority;
                best_coverage = coverage;
            }
        }

        best_index
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedBiome {
    pub name: Arc<str>,
    pub surface_block: BlockId,
    pub soil_block: BlockId,
    pub deep_block: BlockId,
    pub ocean_floor_block: BlockId,
    pub grass_color: [u8; 3],
    pub foliage_color: [u8; 3],
    pub height_offset: f32,
    pub roughness_multiplier: f32,
    priority: i32,
    temperature_min: f32,
    temperature_max: f32,
    humidity_min: f32,
    humidity_max: f32,
    altitude_constraint: RangeConstraint,
    continentalness_constraint: RangeConstraint,
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

        let ocean_floor_block_name = if definition.ocean_floor_block.is_empty() {
            &definition.soil_block
        } else {
            &definition.ocean_floor_block
        };

        Ok(Self {
            name: Arc::<str>::from(definition.name),
            priority: definition.priority,
            temperature_min,
            temperature_max,
            humidity_min,
            humidity_max,
            altitude_constraint: RangeConstraint::from_altitude_bounds(
                definition.altitude_min,
                definition.altitude_max,
            ),
            continentalness_constraint: RangeConstraint::from_unit_bounds(
                definition.continentalness_min,
                definition.continentalness_max,
            ),
            surface_block: resolve_block(&definition.surface_block, block_registry)?,
            soil_block: resolve_block(&definition.soil_block, block_registry)?,
            deep_block: resolve_block(&definition.deep_block, block_registry)?,
            ocean_floor_block: resolve_block(ocean_floor_block_name, block_registry)?,
            grass_color: definition.grass_color,
            foliage_color: definition.foliage_color,
            height_offset: definition.height_offset,
            roughness_multiplier,
        })
    }

    #[inline]
    fn matches(&self, point: BiomeSamplePoint) -> bool {
        self.temperature_min <= point.temperature
            && point.temperature <= self.temperature_max
            && self.humidity_min <= point.humidity
            && point.humidity <= self.humidity_max
            && self.altitude_constraint.matches(point.altitude)
            && self.continentalness_constraint.matches(point.continentalness)
    }

    fn blend_weight(&self, point: BiomeSamplePoint, blend_radius: f32) -> f32 {
        let temperature_weight = smooth_range_weight(
            point.temperature,
            self.temperature_min,
            self.temperature_max,
            blend_radius,
        );
        let humidity_weight =
            smooth_range_weight(point.humidity, self.humidity_min, self.humidity_max, blend_radius);
        let altitude_weight =
            self.altitude_constraint.altitude_blend_weight(point.altitude, blend_radius);
        let continentalness_weight =
            self.continentalness_constraint.blend_weight(point.continentalness, blend_radius);

        temperature_weight * humidity_weight * altitude_weight * continentalness_weight
    }

    fn coverage_score(&self) -> f32 {
        let temperature_span = (self.temperature_max - self.temperature_min).max(f32::EPSILON);
        let humidity_span = (self.humidity_max - self.humidity_min).max(f32::EPSILON);
        let altitude_span = self.altitude_constraint.altitude_specificity_span();
        let continentalness_span = self.continentalness_constraint.unit_specificity_span();

        temperature_span * humidity_span * altitude_span * continentalness_span
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct RangeConstraint {
    min: Option<f32>,
    max: Option<f32>,
}

impl RangeConstraint {
    const fn unbounded() -> Self {
        Self { min: None, max: None }
    }

    fn bounded(min: f32, max: f32) -> Self {
        Self { min: Some(min.min(max)), max: Some(min.max(max)) }
    }

    fn from_unit_bounds(min: Option<f32>, max: Option<f32>) -> Self {
        if min.is_none() && max.is_none() {
            return Self::bounded(0.0, 1.0);
        }

        let resolved_min = min.unwrap_or(0.0).clamp(0.0, 1.0);
        let resolved_max = max.unwrap_or(1.0).clamp(0.0, 1.0);
        Self::bounded(resolved_min, resolved_max)
    }

    fn from_altitude_bounds(min: Option<f32>, max: Option<f32>) -> Self {
        match (min, max) {
            (None, None) => Self::unbounded(),
            (Some(min), Some(max)) => Self::bounded(min, max),
            (Some(min), None) => Self { min: Some(min), max: None },
            (None, Some(max)) => Self { min: None, max: Some(max) },
        }
    }

    #[inline]
    fn matches(&self, value: f32) -> bool {
        self.min.is_none_or(|min| min <= value) && self.max.is_none_or(|max| value <= max)
    }

    fn blend_weight(&self, value: f32, blend_radius: f32) -> f32 {
        match (self.min, self.max) {
            (None, None) => 1.0,
            (Some(min), Some(max)) => smooth_range_weight(value, min, max, blend_radius),
            (Some(min), None) => smooth_lower_bound_weight(value, min, blend_radius),
            (None, Some(max)) => smooth_upper_bound_weight(value, max, blend_radius),
        }
    }

    fn altitude_blend_weight(&self, value: f32, blend_radius: f32) -> f32 {
        if blend_radius <= f32::EPSILON {
            return self.matches(value) as u8 as f32;
        }

        let altitude_edge = (DEFAULT_ALTITUDE_SPAN * blend_radius * 0.5).max(1.0);

        match (self.min, self.max) {
            (None, None) => 1.0,
            (Some(min), Some(max)) => {
                let scaled_edge = ((max - min).abs() * blend_radius).max(1.0);
                smooth_range_weight(value, min, max, scaled_edge)
            }
            (Some(min), None) => smooth_lower_bound_weight(value, min, altitude_edge),
            (None, Some(max)) => smooth_upper_bound_weight(value, max, altitude_edge),
        }
    }

    fn unit_specificity_span(&self) -> f32 {
        match (self.min, self.max) {
            (Some(min), Some(max)) => (max - min).max(f32::EPSILON),
            _ => 1.0,
        }
    }

    fn altitude_specificity_span(&self) -> f32 {
        match (self.min, self.max) {
            (Some(min), Some(max)) => ((max - min).max(1.0) / DEFAULT_ALTITUDE_SPAN).min(1.0),
            (Some(_), None) | (None, Some(_)) => 0.5,
            (None, None) => 1.0,
        }
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
    #[serde(default)]
    altitude_min: Option<f32>,
    #[serde(default)]
    altitude_max: Option<f32>,
    #[serde(default)]
    continentalness_min: Option<f32>,
    #[serde(default)]
    continentalness_max: Option<f32>,
    surface_block: String,
    soil_block: String,
    deep_block: String,
    #[serde(default)]
    ocean_floor_block: String,
    #[serde(default = "default_tint_color")]
    grass_color: [u8; 3],
    #[serde(default = "default_tint_color")]
    foliage_color: [u8; 3],
    #[serde(default)]
    height_offset: f32,
    #[serde(default = "default_roughness_multiplier")]
    roughness_multiplier: f32,
}

fn default_roughness_multiplier() -> f32 {
    1.0
}

fn default_tint_color() -> [u8; 3] {
    DEFAULT_TINT_COLOR
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

fn smooth_lower_bound_weight(value: f32, min: f32, blend_radius: f32) -> f32 {
    if blend_radius <= f32::EPSILON {
        return (value >= min) as u8 as f32;
    }

    smoothstep_range(min - blend_radius.max(0.001), min + blend_radius.max(0.001), value)
}

fn smooth_upper_bound_weight(value: f32, max: f32, blend_radius: f32) -> f32 {
    if blend_radius <= f32::EPSILON {
        return (value <= max) as u8 as f32;
    }

    1.0 - smoothstep_range(max - blend_radius.max(0.001), max + blend_radius.max(0.001), value)
}

#[inline]
fn smoothstep_range(edge0: f32, edge1: f32, value: f32) -> f32 {
    if (edge1 - edge0).abs() <= f32::EPSILON {
        return (value >= edge1) as u8 as f32;
    }

    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
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

    fn point(temperature: f32, humidity: f32) -> BiomeSamplePoint {
        BiomeSamplePoint { temperature, humidity, altitude: 48.0, continentalness: 0.5 }
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
        assert_eq!(table.sample(point(0.9, 0.1)).name.as_ref(), "desert");
        assert_eq!(table.sample(point(0.5, 0.5)).name.as_ref(), "temperate");
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

        assert_eq!(table.sample(point(0.95, 0.95)).name.as_ref(), "fallback");
        assert_eq!(table.sample(point(0.1, 0.1)).name.as_ref(), "cold_dry");
    }

    #[test]
    fn biome_table_uses_altitude_and_continentalness_constraints() {
        let table = biome_table_from_toml(
            r#"
fallback_biome = "plains"

[[biomes]]
name = "plains"
priority = 0
temperature_min = 0.0
temperature_max = 1.0
humidity_min = 0.0
humidity_max = 1.0
surface_block = "grass"
soil_block = "dirt"
deep_block = "stone"

[[biomes]]
name = "snowy_peaks"
priority = 100
temperature_min = 0.0
temperature_max = 1.0
humidity_min = 0.0
humidity_max = 1.0
altitude_min = 90.0
altitude_max = 256.0
continentalness_min = 0.75
continentalness_max = 1.0
surface_block = "stone"
soil_block = "stone"
deep_block = "stone"
"#,
        );

        let lowland_point = BiomeSamplePoint {
            temperature: 0.6,
            humidity: 0.6,
            altitude: 64.0,
            continentalness: 0.8,
        };
        let peak_point = BiomeSamplePoint {
            temperature: 0.8,
            humidity: 0.5,
            altitude: 128.0,
            continentalness: 0.92,
        };
        let coastal_point = BiomeSamplePoint {
            temperature: 0.8,
            humidity: 0.5,
            altitude: 128.0,
            continentalness: 0.45,
        };

        assert_eq!(table.sample(lowland_point).name.as_ref(), "plains");
        assert_eq!(table.sample(peak_point).name.as_ref(), "snowy_peaks");
        assert_eq!(table.sample(coastal_point).name.as_ref(), "plains");
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
name = "dry_highlands"
priority = 10
temperature_min = 0.5
temperature_max = 1.0
humidity_min = 0.0
humidity_max = 0.3
altitude_min = 60.0
altitude_max = 120.0
continentalness_min = 0.55
continentalness_max = 1.0
surface_block = "dirt"
soil_block = "dirt"
deep_block = "stone"
height_offset = 12.0
roughness_multiplier = 2.0
"#,
        );

        let blend = table.sample_blended(
            BiomeSamplePoint {
                temperature: 0.6,
                humidity: 0.28,
                altitude: 62.0,
                continentalness: 0.58,
            },
            0.10,
        );
        assert_eq!(blend.dominant.name.as_ref(), "dry_highlands");
        assert!(blend.height_offset > 0.0 && blend.height_offset < 12.0);
        assert!(blend.roughness_multiplier > 1.0 && blend.roughness_multiplier < 2.0);
    }

    #[test]
    fn biome_table_loads_optional_grass_and_foliage_colors() {
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
grass_color = [120, 180, 80]
foliage_color = [90, 140, 60]
"#,
        );

        let biome = table.sample(point(0.5, 0.5));
        assert_eq!(biome.grass_color, [120, 180, 80]);
        assert_eq!(biome.foliage_color, [90, 140, 60]);
    }
}
