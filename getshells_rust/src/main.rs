#![feature(maybe_uninit_slice)]
#![feature(new_uninit)]
#![feature(read_buf)]
#![feature(hash_raw_entry)]
use std::{
    fmt::Display,
    fs::File,
    io::{stdout, Write},
};

use bstr::ByteSlice;
// use ahash::AHashMap;
// use bstr::ByteSlice;
use memchr::{memchr_iter, memrchr};
use memmap2::Mmap;

// Bad and unsafe, but very fast shell associative store. Stolen from C.
struct Shells {}

fn main() {
    const FILE: &str = "passwd";

    let file = File::open(FILE).expect("failed to read {FILE}");
    let mapped = unsafe { Mmap::map(&file).unwrap() };
    // let mut hs = AHashMap::with_capacity(32); // Initial capacity 32 performed the best at the time
    //                                           // of testing, probably a fragile optimization

    const ENTRIES: usize = 128;
    type COUNT = u64;

    struct BadHash<'a> {
        values: Vec<(Option<&'a [u8]>, COUNT)>,
    }
    impl<'a> BadHash<'a> {
        pub fn new() -> Self {
            BadHash {
                values: Vec::from_iter((0..ENTRIES).map(|_| (None, 0))),
            }
        }
        pub fn get(&mut self, key: &'a [u8]) -> &mut COUNT {
            let id = Self::id(key);
            let val = &mut self.values[id];
            val.0 = Some(key);
            &mut val.1
        }
        fn id(key: &[u8]) -> usize {
            let len = key.len();
            (key[len - 3] as usize ^ (len + key[len - 4] as usize)) & 0xabcdff
        }
        fn into_vec(self) -> Vec<(Option<&'a [u8]>, COUNT)> {
            self.values
        }
    }

    let mut map = BadHash::new();
    let mut stdout = stdout().lock();
    let mut start = 0;
    memchr_iter(b'\n', &mapped).for_each(|end| {
        let line = unsafe { mapped.get_unchecked(start..end) };
        let Some(colon_idx) = memrchr(b':', line).map(|x|x+1) else {
            return ()
        };
        let shell = unsafe { line.get_unchecked(colon_idx..) };

        *map.get(shell) += 1;

        start = end + 1;
    });

    map.into_vec().into_iter().for_each(|(name, count)| {
        if let Some(name) = name {
            let _ = stdout.write_fmt(format_args!("{}: {count}\n", UnsafeBytes(&name)));
        }
    })
}

#[repr(transparent)]
struct UnsafeBytes<'a>(&'a [u8]);
impl<'a> Display for UnsafeBytes<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(unsafe { std::str::from_utf8_unchecked(&self.0) })
    }
}
