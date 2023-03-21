#![feature(hash_raw_entry)]
use std::{
    any::Any,
    fmt::Display,
    fs::OpenOptions,
    io::{self, stdout, Write},
    thread::scope,
};

use ahash::AHashMap;
use bstr::ByteSlice;
use memchr::{memchr_iter, memrchr};
use memmap2::{Mmap, MmapOptions};

const PATH: &str = "passwd";

const LINE_FEED: u8 = b'\n';

const EFFICIENT_CORE_DIVISOR: usize = 6;

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

fn main() {
    let mut args = std::env::args().skip(1);

    let file = OpenOptions::new().read(true).open(PATH).unwrap();
    let mapped = unsafe { MmapOptions::new().map(&file).unwrap() };

    let thread_count = match args.next().map(|x| x.parse::<u64>()) {
        Some(Ok(n)) => {
            if n == 0 {
                eprintln!("Thread count(arg1) cannot be zero");
                std::process::exit(2)
            }
            n
        }
        Some(Err(err)) => panic!("Failed to parse the first argument(thread count),{err}"),
        None => {
            let mut cores = num_cpus::get_physical();
            if cores > 6 {
                let remainder = cores % EFFICIENT_CORE_DIVISOR;
                if remainder != 0 {
                    cores = cores - remainder + EFFICIENT_CORE_DIVISOR
                }
            };
            cores as u64
        }
    };

    let _ = mapped.advise(memmap2::Advice::WillNeed);
    // let _ = mapped.lock();

    let thread_configs = ThreadConfig::generate_chunked(&mapped, thread_count, LINE_FEED).unwrap();

    let map = scope(|s| {
        let mapped = &mapped;
        let threads: Vec<_> = thread_configs
            .into_iter()
            .map(|config| s.spawn(move || config.run(mapped)))
            .collect();
        let mut threads = threads.into_iter();
        let mut map = threads.next().unwrap().join().unwrap().into_vec();
        threads
            .try_for_each(|handle| -> Result<(), Box<dyn Any + Send + 'static>> {
                handle
                    .join()?
                    .into_vec()
                    .into_iter()
                    .enumerate()
                    .for_each(|(i, (k, v))| {
                        if let Some(k) = k {
                            let entry = &mut map[i];
                            entry.1 += v;
                            entry.0 = Some(k);
                        }
                    });
                Ok(())
            })
            .unwrap();
        map
    });

    let mut stdout = stdout().lock();
    map.into_iter().for_each(|(name, count)| {
        if let Some(name) = name {
            let _ = stdout.write_fmt(format_args!("{}: {count}\n", UnsafeBytes(&name)));
        }
    });
    // hashmap.into_iter().for_each(|(shell, count)| {
    //     let _ = stdout.write_fmt(format_args!("{}: {count}\n", UnsafeBytes(&shell)));
    // });
}

#[repr(transparent)]
struct UnsafeBytes<'a>(&'a [u8]);
impl<'a> Display for UnsafeBytes<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(unsafe { std::str::from_utf8_unchecked(&self.0) })
    }
}

#[derive(Debug)]
struct ThreadConfig {
    start: u64,
    length: u64,
}

impl ThreadConfig {
    pub fn run(self, mapped: &Mmap) -> BadHash {
        let (start, length) = (self.start as usize, self.length as usize);
        let _ = mapped.advise_range(memmap2::Advice::Sequential, start, length);
        let mut map = BadHash::new();

        let owned: &[u8] = unsafe { mapped.get_unchecked(start..start + length) };
        let mut start = 0;
        memchr_iter(b'\n', &owned).for_each(|end| {
            let line = unsafe { owned.get_unchecked(start..end) };
            let Some(colon_idx) = memrchr(b':', line).map(|x|x+1) else {
            return ()
        };
            let shell = unsafe { line.get_unchecked(colon_idx..) };

            *map.get(shell) += 1;

            start = end + 1;
        });
        map
    }

    pub fn generate_chunked(map: &Mmap, thread_count: u64, sep: u8) -> std::io::Result<Vec<Self>> {
        let size = (map.len().max(1) - 1) as u64;

        let chunk_size = size / thread_count;

        let mut thread_configs = Vec::with_capacity(thread_count as usize);
        let mut last_end = 0u64;
        for _ in 0..thread_count {
            let start = last_end;
            if start >= size {
                break;
            }
            let chunk_len = chunk_size.min(size - start);
            let advise_start = (start + chunk_len) as usize;
            let owned = unsafe { map.get_unchecked(advise_start..) };
            let offset = owned.find_byte(sep).unwrap_or(0) as u64;
            let length = offset + chunk_len;
            thread_configs.push(ThreadConfig { start, length });
            last_end = start + length + 1;
        }
        Ok(thread_configs)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs::File,
        io::{Cursor, Seek},
    };

    use memmap2::Mmap;

    use crate::ThreadConfig;

    #[test]
    fn thread_configs() {
        const COUNT: u64 = 100;
        let file = File::open("Cargo.toml").unwrap();
        let size = file.seek(std::io::SeekFrom::End(0)).unwrap();
        let _ = file.seek(std::io::SeekFrom::Start(0));
        let map = unsafe { Mmap::map(&file).unwrap() };
        for threads in 1..COUNT {
            // let input = Cursor::new(&input);
            let thread_configs = ThreadConfig::generate_chunked(&map, threads, b'0').unwrap();
            assert_eq!(
                thread_configs.iter().map(|x| x.length).sum::<usize>(),
                size,
                "All lengths have to add up to the total size"
            );
            assert_eq!(
                {
                    let tc = thread_configs.last().unwrap();
                    tc.start + tc.length
                },
                size,
                "The start position of the last thread config and its length have to add up to the total size"
            )
        }
    }
}
