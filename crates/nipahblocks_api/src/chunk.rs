use noise::NoiseFn;
use serde::{Deserialize, Serialize};

use crate::Position;

#[derive(Debug, Serialize, Deserialize, Clone, Copy, Eq, PartialEq, Hash)]
pub struct ChunkId {
    pub x: i16,
    pub y: i16,
}

impl ChunkId {
    fn starting_position(&self) -> Position {
        let width = Chunk::WIDTH as f32;
        let half_width = Chunk::HALF_WIDTH as f32;
        Position {
            x: self.x as f32 * width - half_width,
            y: self.y as f32 * width - half_width,
            z: 0.0,
        }
    }
}

impl From<Position> for ChunkId {
    fn from(value: Position) -> Self {
        let width = Chunk::WIDTH as f32;
        let half_width = Chunk::HALF_WIDTH as f32;
        ChunkId {
            x: ((value.x + half_width) / width).floor() as i16,
            y: ((value.y + half_width) / width).floor() as i16,
        }
    }
}

pub type BlockId = u8;

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct Block(BlockId);

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Chunk {
    id: ChunkId,
    blocks: Vec<Option<Block>>,
}

impl Chunk {
    pub const WIDTH: usize = 16;
    pub const HALF_WIDTH: usize = Self::WIDTH / 2;
    pub const HEIGHT: usize = 256;
    pub const SIZE: usize = Self::WIDTH.pow(2) * Self::HEIGHT;

    pub fn get_block_index(x: usize, y: usize, z: usize) -> usize {
        debug_assert!(x < Self::WIDTH);
        debug_assert!(y < Self::WIDTH);
        debug_assert!(z < Self::HEIGHT);
        z * Self::WIDTH.pow(2) + y * Self::WIDTH + x
    }

    pub fn new(noise: &impl NoiseFn<f64, 2>, chunk_id: ChunkId) -> Self {
        let mut blocks = vec![None; Self::SIZE];
        const MIN_TERRAIN_HEIGHT: usize = 64;
        const MAX_TERRAIN_HEIGHT: usize = 128;
        const MAX_TERRAIN_AMPLITUDE: f32 = (MAX_TERRAIN_HEIGHT - MIN_TERRAIN_HEIGHT) as f32;
        const NOISE_FREQUENCY: f64 = 0.01;
        let start_position = chunk_id.starting_position();
        let global_start_x = start_position.x;
        let global_start_y = start_position.y;
        for (x, y, z, depth) in (0..Self::WIDTH)
            .flat_map(|x| (0..Self::WIDTH).map(move |y| (x, y)))
            .flat_map(|(x, y)| {
                let global_x = global_start_x + x as f32;
                let global_y = global_start_y + y as f32;
                let noise_value = noise.get([
                    global_x as f64 * NOISE_FREQUENCY,
                    global_y as f64 * NOISE_FREQUENCY,
                ]) as f32;
                let noise_value = (noise_value + 1.0) / 2.0;
                let height =
                    MIN_TERRAIN_HEIGHT + (noise_value * MAX_TERRAIN_AMPLITUDE).round() as usize;
                (0..height).map(move |z| (x, y, z, height - z))
            })
        {
            let index = Chunk::get_block_index(x, y, z);
            blocks[index] = Some(match depth {
                0 => Block(3),
                1..=3 => Block(2),
                _ => Block(1),
            })
        }
        Self {
            id: chunk_id,
            blocks,
        }
    }
}
