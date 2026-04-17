pub use alloc::collections::{
    btree_map, btree_set, linked_list, vec_deque, BTreeMap, BTreeSet, BinaryHeap, LinkedList,
    TryReserveError, VecDeque,
};

pub use hashbrown::{hash_set, HashSet};

use core::hash::{BuildHasher, Hasher};

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// [`HashMap`]使用的`BuildHasher`。
///
/// 用`axhal::misc::random()`生成随机种子
#[derive(Clone)]
pub struct RandomState {
    seed_lo: u64,
    seed_hi: u64,
}

impl RandomState {
    /// 使用运行时随机数创建哈希状态。
    pub fn new() -> Self {
        let seed = arceos_api::modules::axhal::misc::random();
        let seed_lo = seed as u64;
        let mut seed_hi = (seed >> 64) as u64;

        // 保证两路 seed 不会退化成“很弱的全零风格”，避免混合效果过差。
        if seed_hi == 0 {
            seed_hi = 0x9e37_79b9_7f4a_7c15;
        }

        Self { seed_lo, seed_hi }
    }
}

impl Default for RandomState {
    fn default() -> Self {
        Self::new()
    }
}

impl BuildHasher for RandomState {
    type Hasher = DefaultHasher;

    fn build_hasher(&self) -> Self::Hasher {
        DefaultHasher::with_keys(self.seed_lo, self.seed_hi)
    }
}

/// 一个轻量的“带密钥”哈希器，作为`axstd::collections`的默认哈希器。
#[derive(Clone)]
pub struct DefaultHasher {
    state: u64,
    key: u64,
}

impl DefaultHasher {
    fn with_keys(seed_lo: u64, seed_hi: u64) -> Self {
        Self {
            state: FNV_OFFSET_BASIS ^ seed_lo,
            key: seed_hi.rotate_left(17) ^ 0x517c_c1b7_2722_0a95,
        }
    }
}

impl Default for DefaultHasher {
    fn default() -> Self {
        Self::with_keys(0, 0)
    }
}

impl Hasher for DefaultHasher {
    fn finish(&self) -> u64 {
        self.state ^ self.key.rotate_left(23)
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.state ^= u64::from(b);
            self.state = self.state.wrapping_mul(FNV_PRIME);
            // 每轮都混入 key 分量，让输入扩散更充分。
            self.state ^= self.key;
            self.key = self.key.rotate_left(7).wrapping_mul(FNV_PRIME);
        }
        self.state ^= self.state >> 32;
    }
}

pub type HashMap<K, V, S = RandomState> = hashbrown::HashMap<K, V, S>;
