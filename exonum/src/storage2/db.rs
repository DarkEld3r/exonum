use std::iter::{Iterator, Peekable};
use std::collections::btree_map::{BTreeMap, Range};
use std::cmp::Ordering;

use super::Result;

use self::NextIterValue::*;


pub type Patch = BTreeMap<Vec<u8>, Change>;
pub type Iter<'a> = Box<Iterator<Item=(&'a [u8], &'a [u8])> + 'a>;

#[derive(Clone)]
pub enum Change {
    Put(Vec<u8>),
    Delete,
}

pub struct Fork {
    snapshot: Box<Snapshot>,
    changes: Patch
}

pub struct ForkIter<'a> {
    snapshot: Peekable<Iter<'a>>,
    changes: Peekable<Range<'a, Vec<u8>, Change>>
}

enum NextIterValue<'a> {
    Stored(&'a [u8], &'a [u8]),
    Replaced(&'a [u8], &'a [u8]),
    Inserted(&'a [u8], &'a [u8]),
    Deleted,
    MissDeleted
}

pub trait Database: Sized + Clone + Send + Sync + 'static {
    fn snapshot(&self) -> Box<Snapshot>;
    fn fork(&self) -> Fork {
        Fork {
            snapshot: self.snapshot(),
            changes: Patch::new(),
        }
    }
    fn merge(&mut self, patch: Patch) -> Result<()>;
}

pub trait Snapshot {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>>;
    fn contains(&self, key: &[u8]) -> Result<bool> {
        Ok(self.get(key)?.is_some())
    }
    fn iter<'a>(&'a self, from: &[u8]) -> Iter<'a>;
}

impl Snapshot for Fork {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        match self.changes.get(key) {
            Some(change) => Ok(match *change {
                Change::Put(ref v) => Some(v.clone()),
                Change::Delete => None,
            }),
            None => self.snapshot.get(key)
        }
    }

    fn contains(&self, key: &[u8]) -> Result<bool> {
        Ok(match self.changes.get(key) {
            Some(change) => match *change {
                Change::Put(..) => true,
                Change::Delete => false,
            },
            None => self.snapshot.get(key)?.is_some()
        })
    }

    fn iter<'a>(&'a self, from: &[u8]) -> Iter<'a> {
        use std::collections::Bound::*;
        let range = (Included(from), Unbounded);
        Box::new(ForkIter {
            snapshot: self.snapshot.iter(from).peekable(),
            changes: self.changes.range::<[u8], _>(range).peekable()
        })
    }
}

impl Fork {
    pub fn put(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.changes.insert(key, Change::Put(value));
    }

    pub fn delete(&mut self, key: Vec<u8>) {
        self.changes.insert(key, Change::Delete);
    }

    pub fn as_snapshot(&self) -> &Snapshot {
        &*self.snapshot
    }

    pub fn into_patch(self) -> Patch {
        self.changes
    }
}

impl<'a> NextIterValue<'a> {
    fn skip_changes(&self) -> bool {
        match *self {
            Replaced(..) | Inserted(..) | Deleted | MissDeleted => true,
            Stored(..) => false,
        }
    }

    fn skip_snapshot(&self) -> bool {
        match *self {
            Stored(..) | Replaced(..) | Deleted => true,
            Inserted(..) | MissDeleted => false
        }
    }

    fn value(&self) -> Option<(&'a [u8], &'a [u8])> {
        match *self {
            Stored(k, v) => Some((k, v)),
            Replaced(k, v) => Some((k, v)),
            Inserted(k, v) => Some((k, v)),
            Deleted | MissDeleted => None
        }
    }
}

impl<'a> Iterator for ForkIter<'a> {
    type Item = (&'a [u8], &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let next = match self.changes.peek() {
                Some(&(k, ref change)) => match self.snapshot.peek() {
                    Some(&(key, ref value)) => match **change {
                        Change::Put(ref v) => match k[..].cmp(key) {
                            Equal => Replaced(k, v),
                            Less => Inserted(k, v),
                            Greater => Stored(key, value)
                        },
                        Change::Delete => match k[..].cmp(key) {
                            Equal => Deleted,
                            Less => MissDeleted,
                            Greater => Stored(key, value)
                        }
                    },
                    None => match **change {
                        Change::Put(ref v) => Inserted(k, v),
                        Change::Delete => MissDeleted,
                    }
                },
                None => match self.snapshot.peek() {
                    Some(&(key, ref value)) => Stored(key, value),
                    None => return None,
                }
            };
            if next.skip_changes() {
                self.changes.next();
            }
            if next.skip_snapshot() {
                self.snapshot.next();
            }
            if let Some(value) = next.value() {
                return Some(value)
            }
        }
    }
}
