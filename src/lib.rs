//! This library provides `VecArray`, an array-like type that holds a number of values backed by
//! a fixed-sized array for no-allocation, quick access.
//! If more items than the array's capacity are stored, it automatically converts into using a `Vec`.
//!
//! This crate similar to the [`staticvec`](https://crates.io/crates/staticvec) crate but has
//! different emphasis: e.g. it can grow beyond the array's capacity, and it compiles on stable.
//!
//! # Implementation
//!
//! A `VecArray` holds data in _either one_ of two storages:
//!
//! 1) a fixed-size array of `MAX_ARRAY_SIZE` (defaults to 4) items, and
//! 2) a dynamic `Vec` with unlimited items.
//!
//! At any time, either one of them (or both) must be empty, depending on the capacity of the array.
//!
//! There is a `len` field containing the total number of items held by the `VecArray`.
//!
//! The fixed-size array is not initialized (i.e. initialized with `MaybeUninit::uninit()`).
//!
//! When `len <= MAX_ARRAY_SIZE`, all elements are stored in the fixed-size array.
//! Array slots `>= len` are `MaybeUninit::uninit()` while slots `< len` are considered actual data.
//! In this scenario, the `Vec` is empty.
//!
//! As soon as we try to push a new item into the `VecArray` that makes the total number exceed
//! `MAX_ARRAY_SIZE`, all the items in the fixed-sized array are taken out, replaced with
//! `MaybeUninit::uninit()` (via `mem::replace`) and pushed into the `Vec`.
//! Then the new item is added to the `Vec`.
//!
//! Therefore, if `len > MAX_ARRAY_SIZE`, then the fixed-size array is considered empty and
//! uninitialized while all data resides in the `Vec`.
//!
//! When popping an item off of the `VecArray`, the reverse is true.  If `len == MAX_ARRAY_SIZE + 1`,
//! after popping the item, all the items residing in the `Vec` are moved back to the fixed-size array.
//! The `Vec` will then be empty.
//!
//! Therefore, if `len <= MAX_ARRAY_SIZE`, data is in the fixed-size array.
//! Otherwise, data is in the `Vec`.
//!
//! # Limitations
//!
//! 1) The constant `MAX_ARRAY_SIZE` must be compiled in, at least until constant generics
//!    land in Rust.  It defaults to 4; to change it, you must clone this repo and modify the code.
//!
//! 2) It automatically converts itself into a `Vec` when over `MAX_ARRAY_SIZE` and back into an array
//!    when the number of items drops below this threshold.  If it so happens that the data is constantly
//!    added and removed from the `VecArray` that straddles this threshold, you'll see excessive
//!    moving and copying of data back-and-forth, plus allocations and deallocations of the `Vec`.

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;

#[cfg(feature = "std")]
use std::{
    fmt,
    hash::{Hash, Hasher},
    iter::FromIterator,
    mem::{self, MaybeUninit},
    ops::{Deref, DerefMut, Index, IndexMut},
};

#[cfg(not(feature = "std"))]
use core::{
    fmt,
    hash::{Hash, Hasher},
    iter::FromIterator,
    mem::{self, MaybeUninit},
    ops::{Deref, DerefMut, Index, IndexMut},
};

#[cfg(not(feature = "std"))]
use alloc::{boxed::Box, vec::Vec};

type ArrayStore<T> = [T; MAX_ARRAY_SIZE];

/// An array-like type that holds a number of values in static storage for no-allocation, quick access.
///
/// # Safety
///
/// This type uses some unsafe code (mainly for uninitialized/unused array slots) for efficiency.
pub struct VecArray<T> {
    /// Total number of values held.
    len: usize,
    /// Fixed-size storage for fast, no-allocation access.
    array_store: [MaybeUninit<T>; MAX_ARRAY_SIZE],
    /// Dynamic storage. For spill-overs.
    vec_store: Vec<T>,
}

/// Maximum slots of fixed-size storage for a `VecArray`.
/// Defaults to 4, which should be enough for many cases and is a good balance between
/// memory consumption (for the fixed-size array) and reduced allocations.
///
/// # Usage Considerations
///
/// To alter this size right now, unfortunately you must clone this repo and modify the code directly.
///
/// This cannot be avoided until constant generics land in Rust.
pub const MAX_ARRAY_SIZE: usize = 4;

impl<T> Drop for VecArray<T> {
    fn drop(&mut self) {
        self.clear();
    }
}

impl<T: Hash> Hash for VecArray<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.iter().for_each(|x| x.hash(state));
    }
}

impl<T> Default for VecArray<T> {
    fn default() -> Self {
        Self {
            len: 0,
            array_store: unsafe { mem::MaybeUninit::uninit().assume_init() },
            vec_store: Vec::new(),
        }
    }
}

impl<T: PartialEq> PartialEq for VecArray<T> {
    fn eq(&self, other: &Self) -> bool {
        if self.len != other.len || self.vec_store != other.vec_store {
            return false;
        }

        if self.len > MAX_ARRAY_SIZE {
            return true;
        }

        unsafe {
            mem::transmute::<_, &ArrayStore<T>>(&self.array_store)
                == mem::transmute::<_, &ArrayStore<T>>(&other.array_store)
        }
    }
}

impl<T: Clone> Clone for VecArray<T> {
    fn clone(&self) -> Self {
        let mut value: Self = Default::default();
        value.len = self.len;

        if self.is_fixed_storage() {
            for x in 0..self.len {
                let item = self.array_store.get(x).unwrap();
                let item_value: &T = unsafe { mem::transmute(item) };
                value.array_store[x] = MaybeUninit::new(item_value.clone());
            }
        } else {
            value.vec_store = self.vec_store.clone();
        }

        value
    }
}

impl<T: Eq> Eq for VecArray<T> {}

impl<T> FromIterator<T> for VecArray<T> {
    fn from_iter<X: IntoIterator<Item = T>>(iter: X) -> Self {
        let mut vec = VecArray::new();

        for x in iter {
            vec.push(x);
        }

        vec
    }
}

impl<T: 'static> IntoIterator for VecArray<T> {
    type Item = T;
    type IntoIter = Box<dyn Iterator<Item = T>>;

    fn into_iter(self) -> Self::IntoIter {
        self.into_iter()
    }
}

impl<T> VecArray<T> {
    /// Create a new `VecArray`.
    pub fn new() -> Self {
        Default::default()
    }

    /// Empty the `VecArray`.
    pub fn clear(&mut self) {
        if self.is_fixed_storage() {
            for x in 0..self.len {
                self.extract_from_array_store(x);
            }
        } else {
            self.vec_store.clear();
        }
        self.len = 0;
    }

    /// Extract a `MaybeUninit` into a concrete initialized type.
    fn extract(value: MaybeUninit<T>) -> T {
        unsafe { value.assume_init() }
    }

    /// Extract an item from the fixed-size array, replacing it with `MaybeUninit::uninit()`.
    ///
    /// # Panics
    ///
    /// Panics if fixed-size storage is not used, or if the `index` is out of bounds.
    fn extract_from_array_store(&mut self, index: usize) -> T {
        if !self.is_fixed_storage() {
            panic!("not fixed storage in VecArray");
        }
        if index >= self.len {
            panic!("index OOB in VecArray");
        }
        Self::extract(mem::replace(
            self.array_store.get_mut(index).unwrap(),
            MaybeUninit::uninit(),
        ))
    }

    /// Set an item into the fixed-size array.
    /// If `drop` is `true`, the original value is extracted then automatically dropped.
    ///
    /// # Panics
    ///
    /// Panics if fixed-size storage is not used, or if the `index` is out of bounds.
    fn set_into_array_store(&mut self, index: usize, value: T, drop: bool) {
        if !self.is_fixed_storage() {
            panic!("not fixed storage in VecArray");
        }
        // Allow setting at most one slot to the right
        if index > self.len {
            panic!("index OOB in VecArray");
        }
        let temp = mem::replace(
            self.array_store.get_mut(index).unwrap(),
            MaybeUninit::new(value),
        );
        if drop {
            // Extract the original value - which will drop it automatically
            Self::extract(temp);
        }
    }

    /// Move item in the fixed-size array into the `Vec`.
    ///
    /// # Panics
    ///
    /// Panics if fixed-size storage is not used, or if the fixed-size storage is not full.
    fn move_fixed_into_vec(&mut self, num: usize) {
        if !self.is_fixed_storage() {
            panic!("not fixed storage in VecArray");
        }
        if self.len != num {
            panic!("fixed storage is not full in VecArray");
        }
        self.vec_store.extend(
            self.array_store
                .iter_mut()
                .take(num)
                .map(|v| mem::replace(v, MaybeUninit::uninit()))
                .map(Self::extract),
        );
    }

    /// Is data stored in fixed-size storage?
    fn is_fixed_storage(&self) -> bool {
        self.len <= MAX_ARRAY_SIZE
    }

    /// Push a new value to the end of this `VecArray`.
    pub fn push<X: Into<T>>(&mut self, value: X) {
        if self.len == MAX_ARRAY_SIZE {
            self.move_fixed_into_vec(MAX_ARRAY_SIZE);
            self.vec_store.push(value.into());
        } else if self.is_fixed_storage() {
            self.set_into_array_store(self.len, value.into(), false);
        } else {
            self.vec_store.push(value.into());
        }
        self.len += 1;
    }

    /// Insert a new value to this `VecArray` at a particular position.
    pub fn insert<X: Into<T>>(&mut self, index: usize, value: X) {
        let index = if index > self.len { self.len } else { index };

        if self.len == MAX_ARRAY_SIZE {
            self.move_fixed_into_vec(MAX_ARRAY_SIZE);
            self.vec_store.insert(index, value.into());
        } else if self.is_fixed_storage() {
            // Move all items one slot to the right
            for x in (index..self.len).rev() {
                let orig_value = self.extract_from_array_store(x);
                self.set_into_array_store(x + 1, orig_value, false);
            }
            self.set_into_array_store(index, value.into(), false);
        } else {
            self.vec_store.insert(index, value.into());
        }
        self.len += 1;
    }

    /// Pop a value from the end of this `VecArray`.
    pub fn pop(&mut self) -> Option<T> {
        if self.is_empty() {
            return None;
        }

        Some(if self.is_fixed_storage() {
            let value = self.extract_from_array_store(self.len - 1);
            self.len -= 1;
            value
        } else {
            let value = self.vec_store.pop().unwrap();
            self.len -= 1;

            // Move back to the fixed array
            if self.vec_store.len() == MAX_ARRAY_SIZE {
                for index in (0..MAX_ARRAY_SIZE).rev() {
                    let item = self.vec_store.pop().unwrap();
                    self.set_into_array_store(index, item, false);
                }
            }

            value
        })
    }

    /// Remove a value from this `VecArray` at a particular position.
    pub fn remove(&mut self, index: usize) -> Option<T> {
        if index >= self.len {
            return None;
        }

        Some(if self.is_fixed_storage() {
            let value = self.extract_from_array_store(index);

            // Move all items one slot to the left
            for x in index + 1..self.len {
                let orig_value = self.extract_from_array_store(x);
                self.set_into_array_store(x - 1, orig_value, false);
            }
            self.len -= 1;

            value
        } else {
            let value = self.vec_store.remove(index);
            self.len -= 1;

            // Move back to the fixed array
            if self.vec_store.len() == MAX_ARRAY_SIZE {
                for index in (0..MAX_ARRAY_SIZE).rev() {
                    let item = self.vec_store.pop().unwrap();
                    self.set_into_array_store(index, item, false);
                }
            }

            value
        })
    }

    /// Get the number of items in this `VecArray`.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Is this `VecArray` empty?
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Get a reference to the item at a particular index.
    pub fn get(&self, index: usize) -> Option<&T> {
        if index >= self.len {
            return None;
        }

        if self.is_fixed_storage() {
            let array_store: &ArrayStore<T> = unsafe { mem::transmute(&self.array_store) };
            array_store.get(index)
        } else {
            self.vec_store.get(index)
        }
    }

    /// Get a mutable reference to the item at a particular index.
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        if index >= self.len {
            return None;
        }

        if self.is_fixed_storage() {
            let array_store: &mut ArrayStore<T> = unsafe { mem::transmute(&mut self.array_store) };
            array_store.get_mut(index)
        } else {
            self.vec_store.get_mut(index)
        }
    }

    /// Get an iterator to entries in the `VecArray`.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        if self.is_fixed_storage() {
            let array_store: &ArrayStore<T> = unsafe { mem::transmute(&self.array_store) };
            array_store[..self.len].iter()
        } else {
            self.vec_store.iter()
        }
    }

    /// Get a mutable iterator to entries in the `VecArray`.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        if self.is_fixed_storage() {
            let array_store: &mut ArrayStore<T> = unsafe { mem::transmute(&mut self.array_store) };
            array_store[..self.len].iter_mut()
        } else {
            self.vec_store.iter_mut()
        }
    }

    /// Move all data into another `VecArray`, overwriting any data there.
    /// The existing `VecArray` is empty after this operation.
    pub fn transfer(&mut self, other: &mut Self) {
        other.clear();

        if self.is_fixed_storage() {
            let array_store2: &mut ArrayStore<T> =
                unsafe { mem::transmute(&mut other.array_store) };

            for x in 0..self.len {
                array_store2[x] = self.extract_from_array_store(x);
            }
        } else {
            other.vec_store = mem::take(&mut self.vec_store);
        }

        other.len = self.len;
        self.len = 0;
    }
}

impl<T: 'static> VecArray<T> {
    /// Get a mutable iterator to entries in the `VecArray`.
    pub fn into_iter(mut self) -> Box<dyn Iterator<Item = T>> {
        if self.is_fixed_storage() {
            let mut it = FixedStorageIterator {
                data: unsafe { mem::MaybeUninit::uninit().assume_init() },
                index: 0,
                limit: self.len,
            };

            for x in 0..self.len {
                it.data[x] =
                    mem::replace(self.array_store.get_mut(x).unwrap(), MaybeUninit::uninit());
            }
            self.len = 0;

            Box::new(it)
        } else {
            Box::new(Vec::from(self).into_iter())
        }
    }
}

/// An iterator that takes control of the fixed-size storage of a `VecArray` and returns its values.
struct FixedStorageIterator<T> {
    data: [MaybeUninit<T>; MAX_ARRAY_SIZE],
    index: usize,
    limit: usize,
}

impl<T> Iterator for FixedStorageIterator<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.limit {
            None
        } else {
            self.index += 1;

            let value = mem::replace(
                self.data.get_mut(self.index - 1).unwrap(),
                MaybeUninit::uninit(),
            );

            unsafe { Some(value.assume_init()) }
        }
    }
}

impl<T: Default> VecArray<T> {
    /// Get the item at a particular index, replacing it with the default.
    pub fn take(&mut self, index: usize) -> Option<T> {
        if index >= self.len {
            return None;
        }

        if self.is_fixed_storage() {
            self.array_store
                .get_mut(index)
                .map(|v| unsafe { mem::transmute(v) })
        } else {
            self.vec_store.get_mut(index)
        }
        .map(mem::take)
    }
}

impl<T: fmt::Debug> fmt::Debug for VecArray<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.iter().collect::<Vec<_>>(), f)
    }
}

impl<T> AsRef<[T]> for VecArray<T> {
    fn as_ref(&self) -> &[T] {
        if self.is_fixed_storage() {
            let array_store: &ArrayStore<T> = unsafe { mem::transmute(&self.array_store) };
            &array_store[..self.len]
        } else {
            &self.vec_store[..]
        }
    }
}

impl<T> AsMut<[T]> for VecArray<T> {
    fn as_mut(&mut self) -> &mut [T] {
        if self.is_fixed_storage() {
            let array_store: &mut ArrayStore<T> = unsafe { mem::transmute(&mut self.array_store) };
            &mut array_store[..self.len]
        } else {
            &mut self.vec_store[..]
        }
    }
}

impl<T> Deref for VecArray<T> {
    type Target = [T];
    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl<T> DerefMut for VecArray<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut()
    }
}

impl<T> Index<usize> for VecArray<T> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        self.get(index).unwrap()
    }
}

impl<T> IndexMut<usize> for VecArray<T> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        self.get_mut(index).unwrap()
    }
}

impl<T> From<VecArray<T>> for Vec<T> {
    fn from(mut value: VecArray<T>) -> Self {
        if value.len <= MAX_ARRAY_SIZE {
            value.move_fixed_into_vec(value.len);
        }
        value.len = 0;

        let mut arr = Self::new();
        arr.append(&mut value.vec_store);
        arr
    }
}

impl<T> From<Vec<T>> for VecArray<T> {
    fn from(mut value: Vec<T>) -> Self {
        let mut arr: Self = Default::default();
        arr.len = value.len();

        if arr.len <= MAX_ARRAY_SIZE {
            for x in (0..arr.len).rev() {
                arr.set_into_array_store(x, value.pop().unwrap(), false);
            }
        } else {
            arr.vec_store = value;
        }

        arr
    }
}
