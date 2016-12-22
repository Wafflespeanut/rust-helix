use std::cmp;
use std::fmt;
use std::marker::PhantomData;
use std::mem;
use std::ops::Range;
use std::usize;

pub trait ReprUsize {
    fn from_usize(usize) -> Self;
    fn into_usize(self) -> usize;
}

impl ReprUsize for bool {
    fn from_usize(i: usize) -> bool {
        match i {
            0 => false,
            1 => true,
            _ => unreachable!(),
        }
    }

    fn into_usize(self) -> usize { self as usize }
}

impl ReprUsize for char {
    fn from_usize(i: usize) -> char { i as u8 as char }
    fn into_usize(self) -> usize { self as u8 as usize }
}

macro_rules! impl_predefined_type {
    ($ty: ty) => {
        impl ReprUsize for $ty {
            fn from_usize(i: usize) -> $ty { i as $ty }
            fn into_usize(self) -> usize { self as usize }
        }
    };
}

impl_predefined_type!(u8);
impl_predefined_type!(u16);
impl_predefined_type!(u32);
impl_predefined_type!(u64);
impl_predefined_type!(usize);
impl_predefined_type!(i8);
impl_predefined_type!(i16);
impl_predefined_type!(i32);
impl_predefined_type!(i64);
impl_predefined_type!(isize);
impl_predefined_type!(f32);
impl_predefined_type!(f64);

#[derive(Clone, Hash)]
pub struct BitsVec<T: ReprUsize> {
    inner: Vec<usize>,
    units: usize,
    bits: usize,
    max_bits: usize,
    leftover: usize,
    _marker: PhantomData<T>,
}

impl<T: ReprUsize> BitsVec<T> {
    pub fn new(bits: usize) -> BitsVec<T> {
        let max = usize::MAX.count_ones() as usize;
        // We can store more bits, but then we might need BigInt to get them out!
        assert!(bits < max, "[new] cannot hold more than {} bits at a time", max - 1);

        BitsVec {
            inner: vec![0],
            units: 0,
            bits: bits,
            max_bits: max,
            leftover: max,
            _marker: PhantomData,
        }
    }

    pub fn with_capacity(bits: usize, capacity: usize) -> BitsVec<T> {
        let mut vec = BitsVec::new(bits);
        vec.reserve(capacity);
        vec
    }

    pub fn push(&mut self, value: T) {
        let mut value = value.into_usize();
        assert!(value >> self.bits == 0,
                "[push] input size is more than allowed size ({} >= {})", value, 2usize.pow(self.bits as u32));

        let mut idx = self.inner.len() - 1;
        let shift;

        if self.leftover < self.bits {
            let left = self.bits - self.leftover;
            self.inner[idx] |= value >> left;
            if self.leftover != 0 {     // special case, in which masking would result in zero!
                value &= (1 << left) - 1;
            }

            self.inner.push(0);
            self.leftover = self.max_bits - left;
            shift = self.max_bits - left;
            idx += 1;
        } else {
            shift = self.leftover - self.bits;
            self.leftover -= self.bits;
        }

        value <<= shift;
        self.inner[idx] |= value;
        self.units += 1;
    }

    pub fn get(&self, i: usize) -> T {
        assert!(i < self.units, "[get] index out of bounds ({} >= {})", i, self.units);
        self.checked_get(i).unwrap()
    }

    pub fn checked_get(&self, i: usize) -> Option<T> {
        if i >= self.units {
            return None
        }

        let idx = i * self.bits / self.max_bits;
        let bits = (i * self.bits) % self.max_bits;
        let diff = self.max_bits - bits;
        let mut val = self.inner[idx];
        if bits != 0 {
            val &= (1 << diff) - 1;
        }

        if diff >= self.bits {
            Some(T::from_usize(val >> (diff - self.bits)))
        } else {
            let shift = self.bits - diff;
            let out = (val << shift) | (self.inner[idx + 1] >> (self.max_bits - shift));
            Some(T::from_usize(out))
        }
    }

    pub fn set(&mut self, i: usize, value: T) {
        let value = value.into_usize();
        assert!(i < self.units, "[set] index out of bounds ({} >= {})", i, self.units);
        assert!(value >> self.bits == 0,
                "[set] input size is more than allowed size ({} >= {})", value, 2usize.pow(self.bits as u32));

        let idx = i * self.bits / self.max_bits;
        let bits = (i * self.bits) % self.max_bits;
        let diff = self.max_bits - bits;
        let mut val = self.inner[idx];

        if diff >= self.bits {
            let shift = diff - self.bits;
            let last = val & ((1 << shift) - 1);
            let mask = if bits == 0 { 0 } else { ((1 << bits) - 1) << diff };   // prevent overflow
            val &= mask;
            val |= value << shift;
            self.inner[idx] = val | last;
        } else {
            let shift = self.bits - diff;
            self.inner[idx] >>= diff;
            self.inner[idx] <<= diff;
            self.inner[idx] |= value >> shift;
            let last = value & ((1 << shift) - 1);
            self.inner[idx + 1] &= (1 << (self.max_bits - shift)) - 1;
            self.inner[idx + 1] |= last << (self.max_bits - shift);
        }
    }

    pub fn len(&self) -> usize {
        self.units
    }

    pub fn is_empty(&self) -> bool {
        self.units == 0
    }

    pub fn reserve(&mut self, additional: usize) {
        self.inner.reserve(additional * self.bits / self.max_bits + 1);
    }

    pub fn clear(&mut self) {
        let bits = self.bits;
        mem::replace(self, BitsVec::new(bits));
    }

    pub fn inner_len(&self) -> usize {
        self.inner.len()
    }

    pub fn iter(&self) -> Iter<T> {
        Iter { vec: self, range: 0..self.units }
    }

    pub fn into_iter(self) -> IntoIter<T> {
        IntoIter { range: 0..self.units, vec: self }
    }
}

impl<T: ReprUsize + Clone> BitsVec<T> {
    pub fn with_elements(bits: usize, length: usize, value: T) -> BitsVec<T> {
        let mut vec = BitsVec::new(bits);
        vec.extend_with_element(length, value);
        vec
    }

    pub fn extend_with_element(&mut self, length: usize, value: T) {
        assert!(length > self.len(), "[extend] final length should be greater than the initial length");
        // Three phases (somewhat inefficient, using safe code and all, but much better than `push`)
        let mut remain = length - self.len();
        self.reserve(remain);

        // 1. Slow push until we get to a common multiple of (self.bits, self.max_bits)
        while self.leftover > 0 {
            self.push(value.clone());
            remain -= 1;
            if remain == 0 {
                return
            }
        }

        // 2. Do the same to a new BitsVec
        let mut temp = BitsVec::new(self.bits);
        temp.reserve(cmp::min(remain, self.max_bits));
        temp.push(value.clone());
        while temp.leftover > 0 && remain > 0 {
            temp.push(value.clone());
        }

        if remain == 0 {
            self.units += temp.units;
            self.inner.extend(&temp.inner);
            return
        }

        // 3. Extend from the new BitsVec
        while remain >= temp.units {
            self.inner.extend(&temp.inner);
            self.units += temp.units;
            remain -= temp.units;
        }

        for _ in 0..remain {
            self.push(value.clone());
        }
    }
}

impl<T: ReprUsize + PartialEq> BitsVec<T> {
    pub fn contains(&self, element: &T) -> bool {
        self.iter().find(|ref i| i == &element).is_some()
    }
}

impl<T: ReprUsize + fmt::Debug> fmt::Debug for BitsVec<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T: ReprUsize> PartialEq for BitsVec<T> {
    fn eq(&self, other: &Self) -> bool {
        if self.units != other.units || self.bits != other.bits {
            return false
        }

        self.inner == other.inner
    }
}

pub struct Iter<'a, T: ReprUsize + 'a> {
    vec: &'a BitsVec<T>,
    range: Range<usize>,
}

impl<'a, T: ReprUsize> IntoIterator for &'a BitsVec<T> {
    type Item = T;
    type IntoIter = Iter<'a, T>;

    fn into_iter(self) -> Iter<'a, T> {
        self.iter()
    }
}

impl<'a, T: ReprUsize> Iterator for Iter<'a, T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        self.range.next().map(|i| self.vec.get(i))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.range.size_hint()
    }
}

impl<'a, T: ReprUsize> DoubleEndedIterator for Iter<'a, T> {
    fn next_back(&mut self) -> Option<T> {
        self.range.next_back().map(|i| self.vec.get(i))
    }
}

impl<'a, T: ReprUsize> ExactSizeIterator for Iter<'a, T> {}

pub struct IntoIter<T: ReprUsize> {
    vec: BitsVec<T>,
    range: Range<usize>,
}

impl<T: ReprUsize> IntoIterator for BitsVec<T> {
    type Item = T;
    type IntoIter = IntoIter<T>;

    fn into_iter(self) -> IntoIter<T> {
        self.into_iter()
    }
}

impl<T: ReprUsize> Iterator for IntoIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        self.range.next().map(|i| self.vec.get(i))
    }
}

impl<T: ReprUsize> DoubleEndedIterator for IntoIter<T> {
    fn next_back(&mut self) -> Option<T> {
        self.range.next_back().map(|i| self.vec.get(i))
    }
}

impl<T: ReprUsize> ExactSizeIterator for IntoIter<T> {}

#[cfg(test)]
mod tests {
    use super::{BitsVec, ReprUsize};
    use std::mem;

    #[repr(usize)]
    #[derive(Clone, Debug, Eq, PartialEq)]
    enum TestEnum {
        Value1,
        Value2,
        Value3,
        Value4,
    }

    impl ReprUsize for TestEnum {
        fn into_usize(self) -> usize { self as usize }
        fn from_usize(i: usize) -> Self { unsafe { mem::transmute(i) } }
    }

    #[test]
    fn test_everything_with_enum() {
        let mut vec = BitsVec::with_elements(4, 16, TestEnum::Value4);
        vec.set(0, TestEnum::Value1);
        vec.set(1, TestEnum::Value2);
        vec.set(2, TestEnum::Value3);
        assert_eq!(vec.len(), 16);
        assert_eq!(vec.inner_len(), 1);
        assert_eq!(vec.get(0), TestEnum::Value1);
        assert_eq!(vec.get(1), TestEnum::Value2);
        assert_eq!(vec.get(2), TestEnum::Value3);
        vec.push(TestEnum::Value4);
        for i in 3..vec.len() {
            assert_eq!(vec.get(i), TestEnum::Value4);
        }
    }
}
