use std::{iter, mem, slice};

use byteorder::{ByteOrder, NativeEndian};
use murmurhash32::murmurhash2;

use super::{Addr, MemoryArena};
use crate::memory_arena::store;
use crate::UnorderedId;

/// Returns the actual memory size in bytes
/// required to create a table with a given capacity.
/// required to create a table of size
pub fn compute_table_size(capacity: usize) -> usize {
    capacity * mem::size_of::<KeyValue>()
}

/// `KeyValue` is the item stored in the hash table.
/// The key is actually a `BytesRef` object stored in an external memory arena.
/// The `value_addr` also points to an address in the memory arena.
#[derive(Copy, Clone)]
struct KeyValue {
    key_value_addr: Addr,
    hash: u32,
    unordered_id: UnorderedId,
}

impl Default for KeyValue {
    fn default() -> Self {
        KeyValue {
            key_value_addr: Addr::null_pointer(),
            hash: 0u32,
            unordered_id: UnorderedId::default(),
        }
    }
}

impl KeyValue {
    fn is_empty(self) -> bool {
        self.key_value_addr.is_null()
    }
}

/// Customized `HashMap` with `&[u8]` keys
///
/// Its main particularity is that rather than storing its
/// keys in the heap, keys are stored in a memory arena
/// inline with the values.
///
/// The quirky API has the benefit of avoiding
/// the computation of the hash of the key twice,
/// or copying the key as long as there is no insert.
pub struct ArenaHashMap {
    table: Box<[KeyValue]>,
    memory_arena: MemoryArena,
    mask: usize,
    occupied: Vec<usize>,
    len: usize,
}

struct QuadraticProbing {
    hash: usize,
    i: usize,
    mask: usize,
}

impl QuadraticProbing {
    #[inline]
    fn compute(hash: usize, mask: usize) -> QuadraticProbing {
        QuadraticProbing { hash, i: 0, mask }
    }

    #[inline]
    fn next_probe(&mut self) -> usize {
        self.i += 1;
        (self.hash + self.i) & self.mask
    }
}

pub struct Iter<'a> {
    hashmap: &'a ArenaHashMap,
    inner: slice::Iter<'a, usize>,
}

impl<'a> Iterator for Iter<'a> {
    type Item = (&'a [u8], Addr, UnorderedId);

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().cloned().map(move |bucket: usize| {
            let kv = self.hashmap.table[bucket];
            let (key, offset): (&'a [u8], Addr) = self.hashmap.get_key_value(kv.key_value_addr);
            (key, offset, kv.unordered_id)
        })
    }
}

/// Returns the greatest power of two lower or equal to `n`.
/// Except if n == 0, in that case, return 1.
///
/// # Panics if n == 0
fn compute_previous_power_of_two(n: usize) -> usize {
    assert!(n > 0);
    let msb = (63u32 - (n as u64).leading_zeros()) as u8;
    1 << msb
}

impl ArenaHashMap {
    pub fn new(table_size: usize) -> ArenaHashMap {
        assert!(table_size > 0);
        let table_size_power_of_2 = compute_previous_power_of_two(table_size);
        let memory_arena = MemoryArena::default();
        let table: Vec<KeyValue> = iter::repeat(KeyValue::default())
            .take(table_size_power_of_2)
            .collect();
        ArenaHashMap {
            table: table.into_boxed_slice(),
            memory_arena,
            mask: table_size_power_of_2 - 1,
            occupied: Vec::with_capacity(table_size_power_of_2 / 2),
            len: 0,
        }
    }

    #[inline]
    pub fn read<Item: Copy + 'static>(&self, addr: Addr) -> Item {
        self.memory_arena.read(addr)
    }

    #[inline]
    fn probe(&self, hash: u32) -> QuadraticProbing {
        QuadraticProbing::compute(hash as usize, self.mask)
    }

    #[inline]
    pub fn mem_usage(&self) -> usize {
        self.table.len() * mem::size_of::<KeyValue>()
    }

    #[inline]
    fn is_saturated(&self) -> bool {
        self.table.len() < self.occupied.len() * 3
    }

    #[inline]
    fn get_key_value(&self, addr: Addr) -> (&[u8], Addr) {
        let data = self.memory_arena.slice_from(addr);
        let key_bytes_len = NativeEndian::read_u16(data) as usize;
        let key_bytes: &[u8] = &data[2..][..key_bytes_len];
        (key_bytes, addr.offset(2u32 + key_bytes_len as u32))
    }

    #[inline]
    fn get_value_addr_if_key_match(&self, target_key: &[u8], addr: Addr) -> Option<Addr> {
        let (stored_key, value_addr) = self.get_key_value(addr);
        if stored_key == target_key {
            Some(value_addr)
        } else {
            None
        }
    }

    #[inline]
    fn set_bucket(&mut self, hash: u32, key_value_addr: Addr, bucket: usize) -> UnorderedId {
        self.occupied.push(bucket);
        let unordered_id = self.len as UnorderedId;
        self.len += 1;
        self.table[bucket] = KeyValue {
            key_value_addr,
            hash,
            unordered_id,
        };
        unordered_id
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn iter(&self) -> Iter<'_> {
        Iter {
            inner: self.occupied.iter(),
            hashmap: self,
        }
    }

    fn resize(&mut self) {
        let new_len = self.table.len() * 2;
        let mask = new_len - 1;
        self.mask = mask;
        let new_table = vec![KeyValue::default(); new_len].into_boxed_slice();
        let old_table = mem::replace(&mut self.table, new_table);
        for old_pos in self.occupied.iter_mut() {
            let key_value: KeyValue = old_table[*old_pos];
            let mut probe = QuadraticProbing::compute(key_value.hash as usize, mask);
            loop {
                let bucket = probe.next_probe();
                if self.table[bucket].is_empty() {
                    *old_pos = bucket;
                    self.table[bucket] = key_value;
                    break;
                }
            }
        }
    }

    /// Get a value associated to a key.
    pub fn get<V>(&self, key: &[u8]) -> Option<V>
    where V: Copy + 'static {
        let hash = murmurhash2(key);
        let mut probe = self.probe(hash);
        loop {
            let bucket = probe.next_probe();
            let kv: KeyValue = self.table[bucket];
            if kv.is_empty() {
                return None;
            } else if kv.hash == hash {
                if let Some(val_addr) = self.get_value_addr_if_key_match(key, kv.key_value_addr) {
                    let v = self.memory_arena.read(val_addr);
                    return Some(v);
                }
            }
        }
    }

    /// `update` create a new entry for a given key if it does not exist
    /// or updates the existing entry.
    ///
    /// The actual logic for this update is define in the `updater`
    /// argument.
    ///
    /// If the key is not present, `updater` will receive `None` and
    /// will be in charge of returning a default value.
    /// If the key already as an associated value, then it will be passed
    /// `Some(previous_value)`.
    pub fn mutate_or_create<V>(
        &mut self,
        key: &[u8],
        mut updater: impl FnMut(Option<V>) -> V,
    ) -> UnorderedId
    where
        V: Copy + 'static,
    {
        if self.is_saturated() {
            self.resize();
        }
        let hash = murmurhash2(key);
        let mut probe = self.probe(hash);
        loop {
            let bucket = probe.next_probe();
            let kv: KeyValue = self.table[bucket];
            if kv.is_empty() {
                // The key does not exist yet.
                let val = updater(None);
                let num_bytes = std::mem::size_of::<u16>() + key.len() + std::mem::size_of::<V>();
                let key_addr = self.memory_arena.allocate_space(num_bytes);
                {
                    let data = self.memory_arena.slice_mut(key_addr, num_bytes);
                    NativeEndian::write_u16(data, key.len() as u16);
                    let stop = 2 + key.len();
                    data[2..stop].copy_from_slice(key);
                    store(&mut data[stop..], val);
                }
                return self.set_bucket(hash, key_addr, bucket);
            } else if kv.hash == hash {
                if let Some(val_addr) = self.get_value_addr_if_key_match(key, kv.key_value_addr) {
                    let v = self.memory_arena.read(val_addr);
                    let new_v = updater(Some(v));
                    self.memory_arena.write_at(val_addr, new_v);
                    return kv.unordered_id;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {

    use std::collections::HashMap;

    use super::{compute_previous_power_of_two, ArenaHashMap};

    #[test]
    fn test_hash_map() {
        let mut hash_map: ArenaHashMap = ArenaHashMap::new(1 << 18);
        hash_map.mutate_or_create(b"abc", |opt_val: Option<u32>| {
            assert_eq!(opt_val, None);
            3u32
        });
        hash_map.mutate_or_create(b"abcd", |opt_val: Option<u32>| {
            assert_eq!(opt_val, None);
            4u32
        });
        hash_map.mutate_or_create(b"abc", |opt_val: Option<u32>| {
            assert_eq!(opt_val, Some(3u32));
            5u32
        });
        let mut vanilla_hash_map = HashMap::new();
        let iter_values = hash_map.iter();
        for (key, addr, _) in iter_values {
            let val: u32 = hash_map.memory_arena.read(addr);
            vanilla_hash_map.insert(key.to_owned(), val);
        }
        assert_eq!(vanilla_hash_map.len(), 2);
    }

    #[test]
    fn test_compute_previous_power_of_two() {
        assert_eq!(compute_previous_power_of_two(8), 8);
        assert_eq!(compute_previous_power_of_two(9), 8);
        assert_eq!(compute_previous_power_of_two(7), 4);
        assert_eq!(compute_previous_power_of_two(u64::MAX as usize), 1 << 63);
    }
}
